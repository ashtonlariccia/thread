//! Language servers: the programs that know a language well enough to
//! complete it and to say what is wrong with it.
//!
//! Thread ships none of them. It knows a list of the common ones, looks for
//! each on `PATH`, and runs the ones that are both installed and switched on
//! (`[lsp] enabled` in the config). What is here is the part that has to be
//! native: finding a server, running it, and carrying whole messages to and
//! from it. What the messages say is the frontend's business.
//!
//! A server runs on this machine. A window working on a remote has none yet.

use std::io::{BufRead, BufReader, Write};
use std::os::windows::io::{AsHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::thread;

use serde::Serialize;

use crate::{Error, Result};

/// No console window for a server that is a console program.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// A server Thread knows how to run.
pub struct Spec {
    /// What `[lsp] enabled` lists it by.
    pub id: &'static str,
    pub name: &'static str,
    /// The program, as it would be typed: found on `PATH`.
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// The languages it serves: the name the bottom bar gives a file, and
    /// the id the protocol knows that language by.
    pub languages: &'static [(&'static str, &'static str)],
    /// A command that installs it, to show beside one that is not installed.
    pub install: &'static str,
    /// Folders to look in besides `PATH`: where its installer puts it without
    /// adding that to `PATH`.
    pub folders: &'static [&'static str],
}

/// Few, for now: the languages being worked in. Another is an entry here.
pub const CATALOG: &[Spec] = &[
    Spec {
        id: "pyright",
        name: "Pyright",
        command: "pyright-langserver",
        args: &["--stdio"],
        languages: &[("Python", "python")],
        install: "npm install -g pyright",
        folders: &[],
    },
    Spec {
        id: "clangd",
        name: "clangd",
        command: "clangd",
        args: &[],
        languages: &[("C", "c"), ("C++", "cpp")],
        install: "winget install LLVM.LLVM",
        // The LLVM installer leaves PATH alone unless asked.
        folders: &["C:/Program Files/LLVM/bin"],
    },
    Spec {
        id: "nil",
        name: "nil",
        command: "nil",
        args: &[],
        languages: &[("Nix", "nix")],
        install: "cargo install --git https://github.com/oxalica/nil nil",
        folders: &[],
    },
];

/// A language a server serves, as the frontend is told of it.
#[derive(Debug, Clone, Serialize)]
pub struct Language {
    /// The name the bottom bar gives a file in it.
    pub name: &'static str,
    /// What the protocol calls it.
    pub id: &'static str,
}

/// A server from the catalog, and whether it is there to be run.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub languages: Vec<Language>,
    pub install: &'static str,
    pub installed: bool,
}

/// Every server Thread knows of, each looked for on `PATH` as this is called.
pub fn catalog() -> Vec<Info> {
    CATALOG
        .iter()
        .map(|spec| Info {
            id: spec.id,
            name: spec.name,
            command: spec.command,
            languages: spec
                .languages
                .iter()
                .map(|&(name, id)| Language { name, id })
                .collect(),
            install: spec.install,
            installed: find(spec).is_some(),
        })
        .collect()
}

/// Where a server's program is: on `PATH`, or in one of the folders its
/// installer is known to use.
fn find(spec: &Spec) -> Option<PathBuf> {
    locate(spec.command).or_else(|| {
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.into());
        locate_in(
            spec.command,
            spec.folders.iter().map(PathBuf::from),
            &extensions,
        )
    })
}

/// Where a program is, the way the shell would find it: in each folder of
/// `PATH`, under each extension in `PATHEXT`.
///
/// Never the bare name. A server installed with npm is three files of that
/// name, and the one with no extension is a shell script for some other
/// system; what runs here is the `.cmd` beside it.
pub fn locate(command: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| DEFAULT_PATHEXT.into());
    locate_in(command, std::env::split_paths(&path), &extensions)
}

fn locate_in(
    command: &str,
    folders: impl Iterator<Item = PathBuf>,
    extensions: &str,
) -> Option<PathBuf> {
    for folder in folders {
        for extension in extensions.split(';').filter(|e| !e.is_empty()) {
            let candidate = folder.join(format!("{command}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// A running language server.
///
/// Dropping it ends the server, and anything the server started: a server
/// installed with npm is a script that starts `node`, and ending the script
/// alone would leave that running.
pub struct Server {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    _job: Option<OwnedHandle>,
}

impl Server {
    /// Start the server with this id, in `cwd`. `on_message` hears each
    /// message it sends, whole; `on_exit` hears once, when it has gone.
    pub fn spawn<M, E>(id: &str, cwd: Option<&Path>, mut on_message: M, on_exit: E) -> Result<Self>
    where
        M: FnMut(&str) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        let spec = CATALOG
            .iter()
            .find(|spec| spec.id == id)
            .ok_or_else(|| Error::Other(anyhow::anyhow!("there is no language server \"{id}\"")))?;
        let program = find(spec).ok_or_else(|| {
            Error::Other(anyhow::anyhow!(
                "{} is not installed. To install it: {}",
                spec.name,
                spec.install
            ))
        })?;

        let mut command = Command::new(&program);
        command
            .args(spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        if let Some(cwd) = cwd.filter(|dir| dir.is_dir()) {
            command.current_dir(cwd);
        }
        let mut child = command.spawn().map_err(|e| {
            Error::Other(anyhow::anyhow!(
                "could not start {}: {e}",
                program.display()
            ))
        })?;

        // Without the job the server still runs; it is only that whatever it
        // started might outlive it.
        let job = child
            .as_handle()
            .try_clone_to_owned()
            .ok()
            .and_then(|process| crate::terminal::job_for(&process).ok());

        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");
        let name = spec.id;

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(message)) => on_message(&message),
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(target: "thread::lsp", "{name}: {e}");
                        break;
                    }
                }
            }
            on_exit();
        });
        // What a server says to the side is for whoever is debugging it. It
        // has to be read either way: a full pipe would stop the server.
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(|line| line.ok()) {
                tracing::debug!(target: "thread::lsp", "{name}: {line}");
            }
        });

        Ok(Self {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            _job: job,
        })
    }

    /// Send one message, framed as the protocol has it.
    pub fn send(&self, message: &str) {
        let Ok(mut stdin) = self.stdin.lock() else {
            return;
        };
        // A server that has gone cannot be written to; its exit says so.
        let _ = write!(stdin, "Content-Length: {}\r\n\r\n{message}", message.len())
            .and_then(|()| stdin.flush());
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The next message: headers, a blank line, and as many bytes as
/// `Content-Length` said. `None` at the end of the stream.
fn read_message(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }

    let length = length.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "a message with no Content-Length",
        )
    })?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn messages_are_read_one_at_a_time() {
        let stream =
            "Content-Length: 2\r\n\r\n{}Content-Type: x\r\ncontent-length: 7\r\n\r\n{\"a\":1}";
        let mut reader = Cursor::new(stream);
        assert_eq!(read_message(&mut reader).unwrap().as_deref(), Some("{}"));
        assert_eq!(
            read_message(&mut reader).unwrap().as_deref(),
            Some("{\"a\":1}")
        );
        assert_eq!(read_message(&mut reader).unwrap(), None);
    }

    /// The length is of bytes, and a message in the middle of which the
    /// stream ends is an error rather than a short message.
    #[test]
    fn a_length_counts_bytes_and_a_short_body_is_an_error() {
        let mut whole = Cursor::new("Content-Length: 4\r\n\r\n\"é\"");
        assert_eq!(read_message(&mut whole).unwrap().as_deref(), Some("\"é\""));

        let mut short = Cursor::new("Content-Length: 10\r\n\r\n{}");
        assert!(read_message(&mut short).is_err());
    }

    #[test]
    fn a_message_without_a_length_is_refused() {
        let mut reader = Cursor::new("Content-Type: x\r\n\r\n{}");
        assert!(read_message(&mut reader).is_err());
    }

    /// The file with no extension is not the program, even when it is there.
    #[test]
    fn a_program_is_found_by_its_extension_and_never_bare() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("server"), "#!/bin/sh").unwrap();
        let folders = || [dir.path().to_path_buf()].into_iter();
        assert_eq!(locate_in("server", folders(), ".EXE;.CMD"), None);

        std::fs::write(dir.path().join("server.cmd"), "@echo off").unwrap();
        let found = locate_in("server", folders(), ".EXE;.CMD").unwrap();
        assert!(found
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("cmd")));
    }

    #[test]
    fn every_server_has_a_name_of_its_own_and_a_language() {
        let mut ids: Vec<_> = CATALOG.iter().map(|spec| spec.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CATALOG.len());
        assert!(CATALOG.iter().all(|spec| !spec.languages.is_empty()));
    }

    #[test]
    fn a_server_that_is_not_installed_is_an_error_naming_it() {
        let error = Server::spawn("no-such-server", None, |_| {}, || {})
            .err()
            .expect("there is no such server")
            .to_string();
        assert!(error.contains("no-such-server"), "got {error}");
    }
}
