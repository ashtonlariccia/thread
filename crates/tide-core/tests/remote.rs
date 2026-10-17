//! `Remote`, against a real SSH and SFTP exchange.
//!
//! The server here is a throwaway one: it listens on loopback, serves a temp
//! folder as its whole filesystem, and answers a shell with an echo. That is
//! enough to prove every request `Remote` makes is one a server understands,
//! without needing an `sshd` on the machine running the tests.
//!
//! `serve` at the bottom runs the same server until it is killed, for
//! pointing a real Tide window at. It is ignored by a normal test run.
//!
//! There is no tmux on Windows, so the server has none either unless it is
//! lent one: with `TIDE_TMUX_WSL` naming a WSL distribution that has it,
//! a command that runs tmux is run there, on a socket of its own and with
//! no config, so that no session of anyone's is touched.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::server::{Auth, ChannelOpenHandle, Msg, Server as _, Session};
use russh::{Channel, ChannelId};
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Status, StatusCode,
};
use tide_core::document::Eol;
use tide_core::remote::{Failure, Remote, Target};
use tide_core::terminal::Terminal;

const USER: &str = "tester";
const PASSWORD: &str = "correct-horse";
const BANNER: &str = "REMOTE-SHELL-READY";
/// What typing `flood` at the shell prints: this many pieces of a kilobyte.
const FLOOD_PIECES: u64 = 4096;
const FLOOD_DONE: &str = "FLOOD-DONE";

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
            raw: HashSet::new(),
            programs: HashMap::new(),
        }
    }
}

/// The socket the borrowed tmux keeps to.
const TMUX_SOCKET: &str = "tide-test";

/// The WSL distribution to borrow a tmux from, if one was named.
fn tmux_distro() -> Option<String> {
    std::env::var("TIDE_TMUX_WSL")
        .ok()
        .filter(|name| !name.is_empty())
}

/// A command for the borrowed tmux, run in WSL. It is handed over as a file:
/// a line with this much quoting in it does not survive being an argument
/// to `wsl.exe`.
fn in_wsl(distro: &str, script: &str) -> std::process::Command {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let script = script
        .replace(
            "exec tmux ",
            &format!("exec tmux -L {TMUX_SOCKET} -f /dev/null "),
        )
        .replace(
            "tmux list-sessions",
            &format!("tmux -L {TMUX_SOCKET} list-sessions"),
        )
        .replace(
            "tmux list-panes",
            &format!("tmux -L {TMUX_SOCKET} list-panes"),
        );
    let file = std::env::temp_dir().join(format!(
        "tide-test-{}-{}.sh",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&file, script).expect("the script");
    // `C:\Users\me` is `/mnt/c/Users/me` from in there.
    let path = file.to_string_lossy().replace('\\', "/");
    let (drive, rest) = path.split_once(":/").expect("a drive");
    let mut command = std::process::Command::new("wsl.exe");
    command.args(["-d", distro, "-e", "sh"]);
    command.arg(format!("/mnt/{}/{rest}", drive.to_lowercase()));
    command
}

struct Connection {
    root: PathBuf,
    /// Channels that have been opened and not yet turned into anything.
    channels: HashMap<ChannelId, Channel<Msg>>,
    /// The channels that are shells, whose input is echoed. The rest of what
    /// arrives is SFTP, which reads its own channel.
    shells: HashSet<ChannelId>,
    /// The channels standing in for a language server: what is sent comes
    /// back as it was, byte for byte.
    raw: HashSet<ChannelId>,
    /// The channels that are a program run in WSL, by what it reads from.
    programs: HashMap<ChannelId, std::process::ChildStdin>,
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
        session.channel_success(id)?;
        let command = String::from_utf8_lossy(command);
        // tmux, if there is one to borrow, and the script that reads how
        // busy the machine is: run for real, its output sent as
        // it comes and the channel closed when it ends.
        let borrowed = ["tmux", "tide-sys:", "kill -s"]
            .iter()
            .any(|mark| command.contains(mark));
        if let (true, Some(distro)) = (borrowed, tmux_distro()) {
            let mut child = in_wsl(&distro, &command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("wsl.exe");
            self.programs
                .insert(id, child.stdin.take().expect("its input"));
            let mut output = child.stdout.take().expect("its output");
            let (handle, runtime) = (session.handle(), tokio::runtime::Handle::current());
            std::thread::spawn(move || {
                let mut buf = [0u8; 16 * 1024];
                while let Ok(n @ 1..) = output.read(&mut buf) {
                    let sent = runtime.block_on(handle.data(id, buf[..n].to_vec()));
                    if sent.is_err() {
                        break;
                    }
                }
                let _ = child.wait();
                let _ = runtime.block_on(handle.close(id));
            });
            return Ok(());
        }
        // Asked which programs there are: there is `clangd`, said among the
        // sort of thing a login shell prints on its way.
        if command.contains("command -v") {
            session.data(id, b"Last login: today\ntide-found:clangd\n".to_vec())?;
            session.close(id)?;
            return Ok(());
        }
        // Asked to run a language server: an echo, after a word from the
        // shell that started it.
        if command.contains("exec clangd") {
            self.raw.insert(id);
            session.data(id, format!("started by: {command}\n").into_bytes())?;
            return Ok(());
        }
        self.shells.insert(id);
        session.data(id, format!("{BANNER} {command}\r\n$ ").into_bytes())?;
        Ok(())
    }

    /// The shell: what is typed comes back, `flood` prints far more than a
    /// screen draws in a moment, and `exit` ends it.
    async fn data(
        &mut self,
        id: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.raw.contains(&id) {
            session.data(id, data.to_vec())?;
            return Ok(());
        }
        if let Some(input) = self.programs.get_mut(&id) {
            let _ = input.write_all(data).and_then(|()| input.flush());
            return Ok(());
        }
        if !self.shells.contains(&id) {
            return Ok(());
        }
        if data.starts_with(b"exit") {
            session.close(id)?;
            return Ok(());
        }
        if data.starts_with(b"flood") {
            for _ in 0..FLOOD_PIECES {
                session.data(id, vec![b'x'; 1024])?;
            }
            session.data(id, FLOOD_DONE.as_bytes().to_vec())?;
            return Ok(());
        }
        let echoed = String::from_utf8_lossy(data).replace('\r', "\r\n$ ");
        session.data(id, echoed.into_bytes())?;
        Ok(())
    }

    /// The other end has finished with a channel: a program on it is told
    /// so the way it would be anywhere, by its input ending.
    async fn channel_eof(
        &mut self,
        id: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.programs.remove(&id);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        id: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.programs.remove(&id);
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
/// The server's key: a new one each time, unless `TIDE_FAKE_SSHD_KEY_SEED`
/// asks for the same one again -- for a server that is stopped and started
/// to stand in for a connection that drops, and has to be the same host
/// when it comes back.
fn host_key() -> russh::keys::PrivateKey {
    use rand::SeedableRng;
    let algorithm = russh::keys::Algorithm::Ed25519;
    let seed = std::env::var("TIDE_FAKE_SSHD_KEY_SEED")
        .ok()
        .and_then(|seed| seed.parse().ok());
    match seed {
        Some(seed) => {
            russh::keys::PrivateKey::random(&mut rand::rngs::StdRng::seed_from_u64(seed), algorithm)
        }
        None => russh::keys::PrivateKey::random(&mut rand::rng(), algorithm),
    }
    .expect("host key")
}

async fn listen(root: &Path, port: u16) -> u16 {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");
    let port = listener.local_addr().expect("address").port();

    let config = Arc::new(russh::server::Config {
        keys: vec![host_key()],
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
        let dir = std::env::temp_dir().join(format!("tide-test-{}", std::process::id()));
        std::env::set_var("TIDE_DATA_DIR", dir);
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

/// Output printed faster than it is drawn stops at the remote, and comes in
/// pieces a window can take: not one for every packet.
#[tokio::test]
async fn a_flood_from_the_remote_waits_for_the_screen() {
    let (_root, remote) = connected().await;
    let (bytes, pieces) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let (done, ended) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
    );

    let terminal = remote
        .shell(
            None,
            80,
            24,
            {
                let (bytes, pieces, done) =
                    (Arc::clone(&bytes), Arc::clone(&pieces), Arc::clone(&done));
                move |chunk| {
                    bytes.fetch_add(chunk.len() as u64, Ordering::SeqCst);
                    pieces.fetch_add(1, Ordering::SeqCst);
                    if chunk.ends_with(FLOOD_DONE.as_bytes()) {
                        done.store(true, Ordering::SeqCst);
                    }
                }
            },
            {
                let ended = Arc::clone(&ended);
                move || ended.store(true, Ordering::SeqCst)
            },
        )
        .await
        .expect("a shell");
    terminal.write(b"flood\r");

    // Nothing is drawn, so it has to stop -- well short of all of it.
    eventually("the shell to be made to wait", || {
        terminal.flow().held_for().is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let stopped_at = bytes.load(Ordering::SeqCst);
    assert!(
        stopped_at < 512 * 1024,
        "{stopped_at} bytes got past a screen that drew none of them"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(bytes.load(Ordering::SeqCst), stopped_at, "still reading");
    assert!(!done.load(Ordering::SeqCst));

    // Drawn as fast as it comes, the rest follows.
    let mut drawn = 0;
    for _ in 0..2000 {
        if done.load(Ordering::SeqCst) {
            break;
        }
        let now = bytes.load(Ordering::SeqCst);
        terminal.flow().drawn(now - drawn);
        drawn = now;
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(done.load(Ordering::SeqCst), "the flood never finished");
    assert!(bytes.load(Ordering::SeqCst) >= FLOOD_PIECES * 1024);
    let pieces = pieces.load(Ordering::SeqCst);
    assert!(pieces < FLOOD_PIECES / 4, "handed over in {pieces} pieces");

    // What is typed still gets through after all that.
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

#[tokio::test]
async fn the_remote_says_which_language_servers_it_has() {
    let (_root, remote) = connected().await;
    let installed: Vec<&str> = remote
        .servers()
        .await
        .into_iter()
        .filter(|server| server.installed)
        .map(|server| server.id)
        .collect();
    assert_eq!(installed, ["clangd"]);
}

#[tokio::test]
async fn a_language_server_runs_on_the_remote_and_ends_when_dropped() {
    use tide_core::lsp::Serving;

    let (_root, remote) = connected().await;
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let ended = Arc::new(AtomicBool::new(false));

    let server = remote
        .serve(
            "clangd",
            Some("/srv/my app"),
            {
                let heard = Arc::clone(&heard);
                move |message| heard.lock().unwrap().push(message.to_owned())
            },
            {
                let ended = Arc::clone(&ended);
                move || ended.store(true, Ordering::SeqCst)
            },
        )
        .await
        .expect("a server");

    // Two messages go out framed, and come back whole and in order, with
    // what the shell said first left out.
    server.send("{\"id\":1}");
    server.send("{\"id\":2}");
    eventually("the messages to come back", || {
        heard.lock().unwrap().len() == 2
    })
    .await;
    assert_eq!(*heard.lock().unwrap(), ["{\"id\":1}", "{\"id\":2}"]);

    drop(server);
    eventually("the dropped server to end", || ended.load(Ordering::SeqCst)).await;
    assert!(!remote.is_closed());
    assert!(remote
        .serve("no-such-server", None, |_| {}, || {})
        .await
        .is_err());
}

/// tmux for real, over the connection: a session made, typed into, left
/// running when the window lets go, and come back to with its past intact.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a tmux in WSL: set TIDE_TMUX_WSL to the distribution"]
async fn a_tmux_session_outlives_the_attachment_to_it() {
    use tide_core::tmux::{Action, Ended, Event, Start, State};

    let distro = tmux_distro().expect("TIDE_TMUX_WSL is not set");
    let end_server = || {
        let _ = in_wsl(&distro, "exec tmux kill-server\n")
            .stderr(std::process::Stdio::null())
            .status();
    };
    end_server();
    let (_root, remote) = connected().await;

    let listing = remote.tmux_sessions().await.expect("a listing");
    assert!(listing.installed);
    assert!(listing.sessions.is_empty(), "{:?}", listing.sessions);

    /// What an attachment has said: the state, each pane's output, the end.
    #[derive(Default)]
    struct Heard {
        state: Option<State>,
        shown: HashMap<u32, Vec<u8>>,
        ended: Option<Ended>,
    }
    let attach = |start: Start| {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let (on_event, on_exit) = (Arc::clone(&heard), Arc::clone(&heard));
        let attaching = remote.tmux(
            start,
            100,
            30,
            200,
            move |event| {
                let mut heard = on_event.lock().unwrap();
                match event {
                    Event::State(state) => heard.state = Some(state),
                    Event::Sizes(_) => {}
                    Event::Output { pane, data } => {
                        heard.shown.entry(pane).or_default().extend(data)
                    }
                    Event::Error(message) => panic!("tmux refused something: {message}"),
                }
            },
            move |ended| on_exit.lock().unwrap().ended = Some(ended),
        );
        async move { (attaching.await.expect("tmux"), heard) }
    };
    let shown = |heard: &Arc<Mutex<Heard>>, pane: u32| {
        let heard = heard.lock().unwrap();
        String::from_utf8_lossy(heard.shown.get(&pane).map_or(&[][..], Vec::as_slice)).into_owned()
    };
    let pane_of = |heard: &Arc<Mutex<Heard>>| {
        let heard = heard.lock().unwrap();
        heard
            .state
            .as_ref()
            .map(|state| (state.session.unwrap(), state.windows[0].panes[0].id))
    };

    // A new session, and something typed into it.
    let (tmux, heard) = attach(Start::New("over ssh".into())).await;
    eventually("attached", || pane_of(&heard).is_some()).await;
    let (session, pane) = pane_of(&heard).unwrap();
    tmux.act(Action::Watch { pane });
    eventually("shown", || shown(&heard, pane).starts_with("\x1bc")).await;
    tmux.act(Action::Write {
        pane,
        text: "echo still-''here\r".into(),
    });
    eventually("echoed", || shown(&heard, pane).contains("still-here")).await;

    // Let go of it. It ends as a detachment, and the session is still there.
    drop(tmux);
    eventually("detached", || heard.lock().unwrap().ended.is_some()).await;
    assert!(heard.lock().unwrap().ended.as_ref().unwrap().attached);
    let listing = remote.tmux_sessions().await.expect("a listing");
    assert_eq!(listing.sessions.len(), 1, "{:?}", listing.sessions);
    assert_eq!(
        (listing.sessions[0].id, listing.sessions[0].attached),
        (session, 0)
    );
    assert_eq!(listing.sessions[0].name, "over ssh");

    // Back to it: what was on the pane is on it still.
    let (tmux, heard) = attach(Start::Attach(session)).await;
    eventually("attached again", || pane_of(&heard).is_some()).await;
    tmux.act(Action::Watch { pane });
    eventually("its past", || shown(&heard, pane).contains("still-here")).await;

    // One that is not there is a reason, not an attachment.
    let (_gone, missing) = attach(Start::Attach(session + 40)).await;
    eventually("refused", || missing.lock().unwrap().ended.is_some()).await;
    let ended = missing.lock().unwrap().ended.clone().unwrap();
    assert!(!ended.attached);
    assert!(ended
        .reason
        .is_some_and(|reason| reason.contains("session")));

    // Ended on purpose, it lets go of us.
    tmux.act(Action::KillSession { session });
    eventually("ended", || heard.lock().unwrap().ended.is_some()).await;
    let listing = remote.tmux_sessions().await.expect("a listing");
    assert!(listing.sessions.is_empty(), "{:?}", listing.sessions);
    end_server();
}

/// One of the editor's terminals, kept in tmux: still there with what was in
/// it after the connection has gone from under it, and gone when its tab is
/// closed -- or when it is forgotten, for a tab closed with no connection.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a tmux in WSL: set TIDE_TMUX_WSL to the distribution"]
async fn a_kept_terminal_outlives_the_connection_and_ends_with_its_tab() {
    let distro = tmux_distro().expect("TIDE_TMUX_WSL is not set");
    // The kept terminals' own server, which the command names for itself.
    let kept_server = |what: &str| {
        let mut command = std::process::Command::new("wsl.exe");
        command.args(["-d", &distro, "-e", "tmux", "-L", "tide"]);
        command.args(what.split(' '));
        command.output().expect("wsl.exe")
    };
    let sessions = || {
        String::from_utf8_lossy(&kept_server("list-sessions -F #{session_name}").stdout)
            .into_owned()
    };
    kept_server("kill-server");

    /// What a terminal has shown, and how it ended: left behind, or over.
    #[derive(Default)]
    struct Heard {
        shown: Vec<u8>,
        left: Option<bool>,
    }
    async fn open(
        remote: &Remote,
        name: &str,
    ) -> (tide_core::remote::KeptTerminal, Arc<Mutex<Heard>>) {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let (on_data, on_exit) = (Arc::clone(&heard), Arc::clone(&heard));
        let terminal = remote
            .kept_shell(
                (name, Some("/tmp")),
                90,
                20,
                200,
                move |data| on_data.lock().unwrap().shown.extend_from_slice(data),
                move |left| on_exit.lock().unwrap().left = Some(left),
            )
            .await
            .expect("a kept terminal");
        (terminal, heard)
    }
    let shown = |heard: &Arc<Mutex<Heard>>| {
        String::from_utf8_lossy(&heard.lock().unwrap().shown).into_owned()
    };
    let left = |heard: &Arc<Mutex<Heard>>| heard.lock().unwrap().left;

    let (_root, remote) = connected().await;
    assert!(remote.prepare_kept(&[]).await, "the remote has tmux");

    // Opened, in the folder asked for, and typed into.
    let (terminal, heard) = open(&remote, "t-kept1").await;
    eventually("shown", || shown(&heard).starts_with("\x1bc")).await;
    terminal.write(b"echo kept-''alive $PWD\r");
    eventually("echoed", || shown(&heard).contains("kept-alive /tmp")).await;

    // The connection goes from under it. It is left, not over.
    remote.disconnect().await;
    eventually("left", || left(&heard).is_some()).await;
    assert_eq!(left(&heard), Some(true));
    drop(terminal);
    assert!(sessions().contains("t-kept1"), "{}", sessions());

    // Back, on a new connection: the same terminal, with what was in it.
    let (_root, remote) = connected().await;
    let (terminal, heard) = open(&remote, "t-kept1").await;
    eventually("its past", || shown(&heard).contains("kept-alive /tmp")).await;

    // Its tab is closed: it is over, and nothing of it is left running.
    terminal.close();
    eventually("over", || left(&heard).is_some()).await;
    assert_eq!(left(&heard), Some(false));
    assert!(!sessions().contains("t-kept1"), "{}", sessions());

    // One whose tab was closed while there was no connection is ended by name.
    let (terminal, heard) = open(&remote, "t-kept2").await;
    eventually("shown", || shown(&heard).starts_with("\x1bc")).await;
    remote.disconnect().await;
    eventually("left", || left(&heard).is_some()).await;
    drop(terminal);
    let (_root, remote) = connected().await;
    assert!(sessions().contains("t-kept2"), "{}", sessions());
    remote.forget_kept(&["t-kept2".to_owned()]).await;
    assert!(!sessions().contains("t-kept2"), "{}", sessions());
    kept_server("kill-server");
}

/// Not a test: the server above, kept running, to point a Tide window at.
///
/// ```text
/// TIDE_FAKE_SSHD_PORT=2222 TIDE_FAKE_SSHD_ROOT=C:\some\folder \
///     cargo test -p tide-core --test remote -- --ignored serve
/// ```
///
/// Sign in as `tester` with the password `correct-horse`.
#[tokio::test]
#[ignore = "runs until killed"]
async fn serve() {
    let port = std::env::var("TIDE_FAKE_SSHD_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(2222);
    let scratch = tempfile::tempdir().unwrap();
    let root = std::env::var_os("TIDE_FAKE_SSHD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| scratch.path().to_owned());

    let port = listen(&root, port).await;
    println!(
        "serving {} on 127.0.0.1:{port} as {USER} / {PASSWORD}",
        root.display()
    );
    std::future::pending::<()>().await;
}
