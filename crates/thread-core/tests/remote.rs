//! `Remote`, against a real SSH and SFTP exchange.
//!
//! The server here is a throwaway one: it listens on loopback, serves a temp
//! folder as its whole filesystem, and answers a shell with an echo. That is
//! enough to prove every request `Remote` makes is one a server understands,
//! without needing an `sshd` on the machine running the tests.
//!
//! `serve` at the bottom runs the same server until it is killed, for
//! pointing a real Thread window at. It is ignored by a normal test run.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};
use thread_core::document::Eol;
use thread_core::remote::{Failure, Remote, Target};
use thread_core::terminal::Terminal;

const USER: &str = "tester";
const PASSWORD: &str = "correct-horse";
const BANNER: &str = "REMOTE-SHELL-READY";

// --- the server -----------------------------------------------------------------

#[derive(Clone)]
struct Server {
    root: PathBuf,
}

impl russh::server::Server for Server {
    type Handler = Connection;

    fn new_client(&mut self, _peer: Option<SocketAddr>) -> Connection {
        Connection {
            root: self.root.clone(),
            channels: HashMap::new(),
            shells: HashSet::new(),
        }
    }
}

struct Connection {
    root: PathBuf,
    /// Channels that have been opened and not yet turned into anything.
    channels: HashMap<ChannelId, Channel<Msg>>,
    /// The channels that are shells, whose input is echoed. The rest of what
    /// arrives is SFTP, which reads its own channel.
    shells: HashSet<ChannelId>,
}

impl russh::server::Handler for Connection {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == USER && password == PASSWORD {
            Auth::Accept
        } else {
            Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            }
        })
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.insert(channel.id(), channel);
        // Awaited: a reply dropped unanswered refuses the channel.
        reply.accept().await;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        match (name, self.channels.remove(&id)) {
            ("sftp", Some(channel)) => {
                session.channel_success(id)?;
                let files = Files {
                    root: self.root.clone(),
                    open: HashMap::new(),
                    next: 0,
                };
                russh_sftp::server::run(channel.into_stream(), files).await;
            }
            _ => session.channel_failure(id)?,
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        id: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(id)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        id: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.shells.insert(id);
        session.channel_success(id)?;
        session.data(id, format!("{BANNER}\r\n$ ").into_bytes())?;
        Ok(())
    }

    /// A shell started with a command: say what it was, so a test can see
    /// that the folder asked for is the one the server was told.
    async fn exec_request(
        &mut self,
        id: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.shells.insert(id);
        session.channel_success(id)?;
        let command = String::from_utf8_lossy(command);
        session.data(id, format!("{BANNER} {command}\r\n$ ").into_bytes())?;
        Ok(())
    }

    /// The shell: what is typed comes back, and `exit` ends it.
    async fn data(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.shells.contains(&id) {
            return Ok(());
        }
        if data.starts_with(b"exit") {
            session.close(id)?;
            return Ok(());
        }
        let echoed = String::from_utf8_lossy(data).replace('\r', "\r\n$ ");
        session.data(id, echoed.into_bytes())?;
        Ok(())
    }
}

/// The SFTP side: `root` served as `/`.
struct Files {
    root: PathBuf,
    open: HashMap<String, Open>,
    next: u32,
}

enum Open {
    File(std::fs::File),
    /// The listing still to be sent; `None` once it has been.
    Dir(Option<Vec<File>>),
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

fn code(error: std::io::Error) -> StatusCode {
    match error.kind() {
        std::io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        std::io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

/// A path as the server spells it: absolute, with `.` and `..` worked out.
fn absolute(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            name => parts.push(name),
        }
    }
    format!("/{}", parts.join("/"))
}

impl Files {
    fn local(&self, path: &str) -> PathBuf {
        self.root.join(absolute(path).trim_start_matches('/'))
    }

    fn handle(&mut self, id: u32, open: Open) -> Handle {
        self.next += 1;
        let handle = self.next.to_string();
        self.open.insert(handle.clone(), open);
        Handle { id, handle }
    }

    fn attrs(&self, id: u32, path: &str) -> Result<Attrs, StatusCode> {
        let meta = std::fs::metadata(self.local(path)).map_err(code)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }
}

impl russh_sftp::server::Handler for Files {
    type Error = StatusCode;

    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, StatusCode> {
        Ok(Name {
            id,
            files: vec![File::dummy(absolute(&path))],
        })
    }

    async fn open(
        &mut self,
        id: u32,
        filename: String,
        flags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let file = std::fs::OpenOptions::from(flags)
            .open(self.local(&filename))
            .map_err(code)?;
        Ok(self.handle(id, Open::File(file)))
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, StatusCode> {
        self.open.remove(&handle);
        Ok(ok(id))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let Some(Open::File(file)) = self.open.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        let mut data = vec![0; len as usize];
        let read = file.read(&mut data).map_err(code)?;
        if read == 0 {
            return Err(StatusCode::Eof);
        }
        data.truncate(read);
        Ok(Data { id, data })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let Some(Open::File(file)) = self.open.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        file.seek(SeekFrom::Start(offset)).map_err(code)?;
        file.write_all(&data).map_err(code)?;
        Ok(ok(id))
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        self.attrs(id, &path)
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        self.attrs(id, &path)
    }

    async fn fstat(&mut self, id: u32, handle: String) -> Result<Attrs, StatusCode> {
        let Some(Open::File(file)) = self.open.get(&handle) else {
            return Err(StatusCode::Failure);
        };
        let meta = file.metadata().map_err(code)?;
        Ok(Attrs {
            id,
            attrs: FileAttributes::from(&meta),
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        let files = std::fs::read_dir(self.local(&path))
            .map_err(code)?
            .flatten()
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                Some(File::new(
                    entry.file_name().to_string_lossy(),
                    FileAttributes::from(&meta),
                ))
            })
            .collect();
        Ok(self.handle(id, Open::Dir(Some(files))))
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        let Some(Open::Dir(listing)) = self.open.get_mut(&handle) else {
            return Err(StatusCode::Failure);
        };
        // All of it at once, and then the end.
        let files = listing.take().ok_or(StatusCode::Eof)?;
        Ok(Name { id, files })
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, StatusCode> {
        std::fs::remove_file(self.local(&filename)).map_err(code)?;
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, StatusCode> {
        std::fs::create_dir(self.local(&path)).map_err(code)?;
        Ok(ok(id))
    }

    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        std::fs::remove_dir(self.local(&path)).map_err(code)?;
        Ok(ok(id))
    }

    async fn rename(&mut self, id: u32, old: String, new: String) -> Result<Status, StatusCode> {
        std::fs::rename(self.local(&old), self.local(&new)).map_err(code)?;
        Ok(ok(id))
    }
}

/// Serve `root` on a loopback port. Returns once it is listening.
async fn listen(root: &Path, port: u16) -> u16 {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();

    let config = Arc::new(russh::server::Config {
        keys: vec![russh::keys::PrivateKey::random(
            &mut rand::rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .expect("host key")],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let mut server = Server {
        root: root.to_owned(),
    };
    tokio::spawn(async move {
        let _ = server.run_on_socket(config, &listener).await;
    });
    port
}

// --- the tests --------------------------------------------------------------------

/// The host keys trusted during a test go to a scratch folder, not the user's.
fn isolate() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("thread-test-{}", std::process::id()));
        std::env::set_var("THREAD_DATA_DIR", dir);
    });
}

fn target(port: u16, password: &str, trust_new_key: bool) -> Target {
    serde_json::from_value(serde_json::json!({
        "host": "127.0.0.1",
        "port": port,
        "username": USER,
        "auth": { "kind": "password", "password": password },
        "trustNewKey": trust_new_key,
    }))
    .expect("a target, as the window sends one")
}

/// A server with an empty folder, and a connection to it that trusts its key.
async fn connected() -> (tempfile::TempDir, Remote) {
    isolate();
    let root = tempfile::tempdir().expect("temp dir");
    let port = listen(root.path(), 0).await;
    let remote = Remote::connect(&target(port, PASSWORD, true))
        .await
        .expect("connect");
    (root, remote)
}

#[tokio::test]
async fn a_new_host_is_asked_about_and_then_remembered() {
    isolate();
    let root = tempfile::tempdir().unwrap();
    let port = listen(root.path(), 0).await;

    let first = Remote::connect(&target(port, PASSWORD, false))
        .await
        .err()
        .expect("an unknown host is not connected to unasked");
    assert_eq!(first.kind, Failure::UnknownHostKey);
    let fingerprint = first.fingerprint.expect("the key to ask about");
    assert!(fingerprint.starts_with("SHA256:"), "got {fingerprint}");

    Remote::connect(&target(port, PASSWORD, true))
        .await
        .expect("trusted, so connected");
    Remote::connect(&target(port, PASSWORD, false))
        .await
        .expect("remembered, so not asked again");
}

#[tokio::test]
async fn a_wrong_password_is_the_server_saying_no() {
    isolate();
    let root = tempfile::tempdir().unwrap();
    let port = listen(root.path(), 0).await;

    let error = Remote::connect(&target(port, "wrong", true))
        .await
        .err()
        .expect("refused");
    assert_eq!(error.kind, Failure::Auth);
}

#[tokio::test]
async fn a_file_is_written_read_back_and_noticed_changing() {
    let (root, remote) = connected().await;
    assert_eq!(remote.home(), "/");

    let written = remote
        .write("/notes.txt", "one\ntwo", Eol::Crlf, false)
        .await
        .expect("write");
    assert_eq!(
        std::fs::read(root.path().join("notes.txt")).unwrap(),
        b"one\r\ntwo"
    );

    let doc = remote.read("/notes.txt").await.expect("read");
    assert_eq!(doc.name, "notes.txt");
    assert_eq!(doc.path, "/notes.txt");
    assert_eq!(doc.text, "one\ntwo");
    assert_eq!(doc.eol, Eol::Crlf);
    assert_eq!(doc.stamp, written, "untouched, so unchanged");
    assert!(written.is_some());

    // Written behind our back, to a different length.
    std::fs::write(root.path().join("notes.txt"), "something else entirely").unwrap();
    assert_ne!(remote.stamp("/notes.txt").await.unwrap(), written);

    // Saving something shorter leaves none of the old text behind it.
    remote
        .write("/notes.txt", "x", Eol::Lf, false)
        .await
        .unwrap();
    assert_eq!(std::fs::read(root.path().join("notes.txt")).unwrap(), b"x");
}

/// "Not there" is an answer; it must not be mistaken for a failure to ask.
#[tokio::test]
async fn a_file_that_is_not_there_has_no_stamp() {
    let (root, remote) = connected().await;
    std::fs::create_dir(root.path().join("src")).unwrap();

    assert_eq!(remote.stamp("/gone.txt").await.unwrap(), None);
    assert_eq!(
        remote.stamp("/src").await.unwrap(),
        None,
        "a folder is not a file"
    );
    assert_eq!(remote.dir_stamp("/gone").await.unwrap(), None);
    assert!(remote.dir_stamp("/src").await.unwrap().is_some());
    assert!(remote.read("/gone.txt").await.is_err());
}

#[tokio::test]
async fn a_folder_lists_folders_first_and_leaves_out_what_is_excluded() {
    let (root, remote) = connected().await;
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join("Zed.md"), "").unwrap();
    std::fs::write(root.path().join("alpha.rs"), "").unwrap();

    let listed = remote.list("/", &[".*".to_owned()]).await.expect("list");
    let names: Vec<_> = listed.iter().map(|e| (e.name.as_str(), e.dir)).collect();
    assert_eq!(
        names,
        [("src", true), ("alpha.rs", false), ("Zed.md", false)]
    );
    assert_eq!(listed[0].path, "/src");

    assert_eq!(remote.list("/", &[]).await.unwrap().len(), 4);
    assert_eq!(remote.resolve("/src/../src/.").await.unwrap(), "/src");
}

#[tokio::test]
async fn the_tree_can_create_rename_and_delete() {
    let (root, remote) = connected().await;
    let on_disk = |path: &str| root.path().join(path);

    assert_eq!(remote.create_dir("/", "src").await.unwrap(), "/src");
    assert_eq!(
        remote.create_file("/src", "a.rs").await.unwrap(),
        "/src/a.rs"
    );
    assert!(on_disk("src/a.rs").is_file());

    // Nothing typed into the tree can destroy what is already there.
    std::fs::write(on_disk("src/a.rs"), "kept").unwrap();
    let error = remote.create_file("/src", "a.rs").await.unwrap_err();
    assert!(error.to_string().contains("already exists"), "got {error}");
    assert!(remote.create_dir("/", "src").await.is_err());
    assert_eq!(std::fs::read(on_disk("src/a.rs")).unwrap(), b"kept");

    assert_eq!(
        remote.rename("/src/a.rs", "b.rs").await.unwrap(),
        "/src/b.rs"
    );
    assert!(on_disk("src/b.rs").is_file() && !on_disk("src/a.rs").exists());

    std::fs::write(on_disk("src/c.rs"), "also kept").unwrap();
    assert!(remote.rename("/src/b.rs", "c.rs").await.is_err());
    assert_eq!(std::fs::read(on_disk("src/c.rs")).unwrap(), b"also kept");

    assert!(remote.create_file("/src", "a/b").await.is_err());

    // A folder goes with everything in it, however deep.
    std::fs::create_dir_all(on_disk("src/deep/deeper")).unwrap();
    std::fs::write(on_disk("src/deep/deeper/x"), "").unwrap();
    remote.delete("/src").await.expect("delete a folder");
    assert!(!on_disk("src").exists());

    std::fs::write(on_disk("lone.txt"), "").unwrap();
    remote.delete("/lone.txt").await.expect("delete a file");
    assert!(!on_disk("lone.txt").exists());
}

/// Wait for `done`, which a server on the other end of a socket makes true.
async fn eventually(what: &str, done: impl Fn() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn a_terminal_is_a_shell_on_the_remote() {
    let (_root, remote) = connected().await;
    let seen = Arc::new(Mutex::new(Vec::<u8>::new()));
    let ended = Arc::new(AtomicBool::new(false));
    let text = {
        let seen = Arc::clone(&seen);
        move || String::from_utf8_lossy(&seen.lock().unwrap()).into_owned()
    };

    let terminal = remote
        .shell(
            Some("/srv/my app"),
            80,
            24,
            {
                let seen = Arc::clone(&seen);
                move |chunk| seen.lock().unwrap().extend_from_slice(chunk)
            },
            {
                let ended = Arc::clone(&ended);
                move || ended.store(true, Ordering::SeqCst)
            },
        )
        .await
        .expect("a shell");

    // Started in the folder asked for, quoted so a space in it survives.
    eventually("the shell to start", || text().contains(BANNER)).await;
    assert!(text().contains("cd '/srv/my app'"), "got {}", text());

    terminal.write(b"echo hello\r");
    eventually("the echo", || text().contains("echo hello")).await;
    terminal.resize(120, 40);

    // A second one, beside it on the same connection.
    let other = remote
        .shell(None, 80, 24, |_| {}, || {})
        .await
        .expect("a second shell");
    drop(other);

    terminal.write(b"exit\r");
    eventually("`exit` to end the terminal", || {
        ended.load(Ordering::SeqCst)
    })
    .await;
}

#[tokio::test]
async fn dropping_a_terminal_ends_it_and_leaves_the_connection_up() {
    let (_root, remote) = connected().await;
    let ended = Arc::new(AtomicBool::new(false));

    let terminal = remote
        .shell(None, 80, 24, |_| {}, {
            let ended = Arc::clone(&ended);
            move || ended.store(true, Ordering::SeqCst)
        })
        .await
        .expect("a shell");
    drop(terminal);

    eventually("the dropped terminal to end", || {
        ended.load(Ordering::SeqCst)
    })
    .await;
    assert!(!remote.is_closed());
    assert!(remote.list("/", &[]).await.is_ok(), "files still work");

    remote.disconnect().await;
    eventually("the connection to close", || remote.is_closed()).await;
}

/// Not a test: the server above, kept running, to point a Thread window at.
///
/// ```text
/// THREAD_FAKE_SSHD_PORT=2222 THREAD_FAKE_SSHD_ROOT=C:\some\folder \
///     cargo test -p thread-core --test remote -- --ignored serve
/// ```
///
/// Sign in as `tester` with the password `correct-horse`.
#[tokio::test]
#[ignore = "runs until killed"]
async fn serve() {
    let port = std::env::var("THREAD_FAKE_SSHD_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(2222);
    let scratch = tempfile::tempdir().unwrap();
    let root = std::env::var_os("THREAD_FAKE_SSHD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.path().to_owned());

    let port = listen(&root, port).await;
    println!(
        "serving {} on 127.0.0.1:{port} as {USER} / {PASSWORD}",
        root.display()
    );
    std::future::pending::<()>().await;
}
