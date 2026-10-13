//! A remote machine, reached over SSH.
//!
//! One connection does everything a window needs of a remote: its files go
//! over SFTP, and each terminal is a shell channel beside it, as is tmux
//! when the window is showing that. Nothing is installed on the other end;
//! anything that runs `sshd` will do.
//!
//! What SFTP cannot do is tell us when a file changes, and it does not need
//! to: open files and unfolded folders are already kept current by asking for
//! their stamps on a timer, and a stamp is one request here as it is one
//! `stat` on a local disk.
//!
//! A real SSH client (`russh`) rather than a wrapper around `ssh.exe`, so a
//! password goes through the protocol itself: whether it worked is the
//! server's answer, not something inferred from watching for a prompt.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handler};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::time::Instant;

use crate::document::{self, Document, Eol, Stamp};
use crate::known_hosts::{KnownHosts, Verdict};
use crate::sysinfo;
use crate::terminal::{Flow, Terminal, FRAME};
use crate::tmux;
use crate::tree::{self, Entry};
use crate::{Error, Result};

/// How long the host has to answer at all. Short: the everyday failure is a
/// machine that is off or on another network, and such a host swallows the
/// connection rather than refusing it, which Windows would otherwise wait
/// twenty seconds over.
const REACH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long the handshake may take once the socket is up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long signing in may take. A server that shakes hands and then goes
/// quiet would otherwise be waited on for good.
const AUTH_TIMEOUT: Duration = Duration::from_secs(20);

/// How to prove who we are.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Auth {
    Password {
        password: String,
    },
    Key {
        /// The *private* key's path, on this machine.
        path: String,
        /// Empty for a key that is not encrypted.
        #[serde(default)]
        passphrase: String,
    },
}

/// Who to connect to, and as whom.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: Auth,
    /// Accept and remember whatever key the server presents. Only ever set
    /// once a person has been shown the fingerprint and agreed to it.
    #[serde(default)]
    pub trust_new_key: bool,
}

impl Target {
    /// `user@host`, with the port when it is not the usual one.
    pub fn label(&self) -> String {
        label(&self.username, &self.host, self.port)
    }
}

pub fn label(username: &str, host: &str, port: u16) -> String {
    if port == 22 {
        format!("{username}@{host}")
    } else {
        format!("{username}@{host}:{port}")
    }
}

/// Why a connection could not be made, as something the window can act on:
/// a wrong password means "try again", a host nothing answers at means
/// "check the address", and a key that needs deciding means "ask".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    /// The server turned the credentials down.
    Auth,
    /// No usable connection: no such name, refused, timed out, not SSH.
    Unreachable,
    /// Signed in, but the server would not give us what we came for.
    Session,
    /// The first sight of this host. Its fingerprint needs agreeing to.
    UnknownHostKey,
    /// The host's key is not the one trusted before.
    HostKeyChanged,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectError {
    pub kind: Failure,
    pub message: String,
    /// The key the server presented, for the question about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// The key trusted before, when it has changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_fingerprint: Option<String>,
}

impl ConnectError {
    pub fn new(kind: Failure, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            fingerprint: None,
            expected_fingerprint: None,
        }
    }
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// A private key found in `~\.ssh`, for the connect dialog to offer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredKey {
    pub path: String,
    pub name: String,
    /// Needs a passphrase, so the dialog can ask for it up front.
    pub encrypted: bool,
}

/// The private keys in `%USERPROFILE%\.ssh`, the kind servers prefer first.
pub fn discover_keys() -> Vec<DiscoveredKey> {
    let Some(home) = std::env::var_os("USERPROFILE") else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(std::path::Path::new(&home).join(".ssh")) else {
        return Vec::new();
    };

    let mut found: Vec<DiscoveredKey> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_file() || name.ends_with(".pub") {
                return None;
            }
            // Only what really is a key: a `known_hosts` or a `config` in the
            // list would be worse than no list.
            let text = std::fs::read_to_string(&path).ok()?;
            if !text.contains("PRIVATE KEY") {
                return None;
            }
            Some(DiscoveredKey {
                path: path.to_string_lossy().into_owned(),
                name,
                // Decoding it with no passphrase says whether one is needed.
                encrypted: russh::keys::decode_secret_key(&text, None).is_err(),
            })
        })
        .collect();

    found.sort_by_key(|key| match () {
        _ if key.name.contains("ed25519") => 0,
        _ if key.name.contains("ecdsa") => 1,
        _ if key.name.contains("rsa") => 2,
        _ => 3,
    });
    found
}

/// Holds the server's key to what [`KnownHosts`] says about it.
///
/// `check_server_key` can only answer yes or no, so when it says no it leaves
/// the reason here for [`Remote::connect`] to turn into an error that carries
/// the fingerprint.
struct VerifyHostKey {
    host: String,
    port: u16,
    trust_new_key: bool,
    seen: Arc<Mutex<Option<(String, Verdict)>>>,
}

impl Handler for VerifyHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = key.fingerprint(russh::keys::HashAlg::Sha256).to_string();
        let known = KnownHosts::load().unwrap_or_default();
        let verdict = known.verdict(&self.host, self.port, &fingerprint);

        if let Ok(mut seen) = self.seen.lock() {
            *seen = Some((fingerprint.clone(), verdict.clone()));
        }

        Ok(match verdict {
            Verdict::Match => true,
            // A person has been shown this fingerprint and agreed to it.
            _ if self.trust_new_key => {
                if let Err(e) = KnownHosts::remember(&self.host, self.port, &fingerprint) {
                    tracing::error!("could not record the host key: {e}");
                }
                true
            }
            _ => false,
        })
    }
}

/// A connection to a remote machine.
///
/// Dropping it closes the connection, and with it every terminal on it.
pub struct Remote {
    handle: client::Handle<VerifyHostKey>,
    sftp: SftpSession,
    /// Where the server puts us: the folder a browse for one starts in.
    home: String,
}

fn refuse(message: impl Into<String>) -> Error {
    Error::Other(anyhow::anyhow!(message.into()))
}

/// The last component of a remote path.
fn name_of(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// The folder a remote path is in.
fn parent_of(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((parent, _)) => parent,
    }
}

fn join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// A name typed for something new on the remote. One component: the tree
/// makes things *in* a folder, so one that wanders elsewhere is a mistake.
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(refuse("A name is needed."));
    }
    if name == "." || name == ".." {
        return Err(refuse(format!(
            "\"{name}\" is not a name that can be used."
        )));
    }
    if let Some(bad) = name.chars().find(|c| *c == '/' || c.is_control()) {
        return Err(refuse(format!("A name cannot contain {bad:?}.")));
    }
    Ok(())
}

fn missing(error: &SftpError) -> bool {
    matches!(error, SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile)
}

fn stamp_of(attrs: &FileAttributes) -> Option<Stamp> {
    Some(Stamp {
        modified: u64::from(attrs.mtime?) * 1000,
        len: attrs.size?,
    })
}

/// Quote a path for the remote shell.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

impl Remote {
    /// Connect, sign in, and open the file channel.
    pub async fn connect(target: &Target) -> std::result::Result<Self, ConnectError> {
        let at = format!("{}:{}", target.host, target.port);
        let config = Arc::new(client::Config {
            // An editor sits idle for hours; that is not a reason to hang up.
            inactivity_timeout: None,
            // But a link that has died should be noticed without anyone
            // having to save a file into it first.
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        });

        // The socket first, by hand, so "is anything there" has its own short
        // deadline. The same stream then goes to the handshake.
        let connecting = tokio::net::TcpStream::connect((target.host.as_str(), target.port));
        let stream = match tokio::time::timeout(REACH_TIMEOUT, connecting).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => {
                return Err(ConnectError::new(
                    Failure::Unreachable,
                    format!("Could not reach {at}: {e}"),
                ))
            }
            Err(_) => {
                return Err(ConnectError::new(
                    Failure::Unreachable,
                    format!(
                        "{at} did not answer within {} seconds. It may be off, on another \
                         network, or behind a firewall.",
                        REACH_TIMEOUT.as_secs()
                    ),
                ))
            }
        };
        // Typing is felt a keystroke at a time; do not let the socket batch it.
        let _ = stream.set_nodelay(true);

        let seen = Arc::new(Mutex::new(None));
        let handler = VerifyHostKey {
            host: target.host.clone(),
            port: target.port,
            trust_new_key: target.trust_new_key,
            seen: Arc::clone(&seen),
        };

        let shaking = client::connect_stream(config, stream, handler);
        let mut handle = match tokio::time::timeout(HANDSHAKE_TIMEOUT, shaking).await {
            Ok(Ok(handle)) => handle,
            Ok(Err(e)) => {
                // A key that was turned down arrives as an ordinary failed
                // connection; what the handler saw says which kind it was.
                let seen = seen.lock().ok().and_then(|seen| seen.clone());
                return Err(match seen {
                    Some((fingerprint, Verdict::Unknown)) => ConnectError {
                        kind: Failure::UnknownHostKey,
                        message: format!("Thread has not connected to {at} before."),
                        fingerprint: Some(fingerprint),
                        expected_fingerprint: None,
                    },
                    Some((fingerprint, Verdict::Changed { expected })) => ConnectError {
                        kind: Failure::HostKeyChanged,
                        message: format!(
                            "The host key for {at} has changed. That happens when a server \
                             is rebuilt, and it is also what someone impersonating it looks \
                             like. Do not go on unless you know why it changed."
                        ),
                        fingerprint: Some(fingerprint),
                        expected_fingerprint: Some(expected),
                    },
                    _ => ConnectError::new(
                        Failure::Unreachable,
                        format!("{at} answered, but not as an SSH server: {e}"),
                    ),
                });
            }
            Err(_) => {
                return Err(ConnectError::new(
                    Failure::Unreachable,
                    format!("{at} answered but never finished the SSH handshake."),
                ))
            }
        };

        let signing_in = authenticate(&mut handle, target);
        let signed_in = match tokio::time::timeout(AUTH_TIMEOUT, signing_in).await {
            Ok(result) => result?,
            Err(_) => {
                return Err(ConnectError::new(
                    Failure::Auth,
                    format!("{at} never answered the attempt to sign in."),
                ))
            }
        };
        if !signed_in {
            return Err(ConnectError::new(
                Failure::Auth,
                match target.auth {
                    Auth::Password { .. } => "Wrong username or password.".to_owned(),
                    Auth::Key { .. } => format!(
                        "The server did not accept that key for {}.",
                        target.username
                    ),
                },
            ));
        }

        let session = |what: &str, e: &dyn std::fmt::Display| {
            ConnectError::new(Failure::Session, format!("Signed in, but {what}: {e}"))
        };
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| session("the server would not open a channel", &e))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| session("the server does not offer SFTP", &e))?;
        let sftp = SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| session("its SFTP service did not answer", &e))?;
        let home = sftp
            .canonicalize(".")
            .await
            .unwrap_or_else(|_| "/".to_owned());

        Ok(Self { handle, sftp, home })
    }

    pub fn home(&self) -> &str {
        &self.home
    }

    /// Whether the connection has ended, by either side or by the network.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed()
    }

    /// Hang up, saying so, rather than letting the socket drop.
    pub async fn disconnect(&self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
    }

    // --- files ----------------------------------------------------------------

    /// A path as the server spells it: absolute, with `.` and `..` resolved.
    pub async fn resolve(&self, path: &str) -> Result<String> {
        self.sftp
            .canonicalize(path)
            .await
            .map_err(|e| refuse(format!("could not find {path}: {e}")))
    }

    /// The file's stamp; `None` if there is no such file.
    ///
    /// An error is something else: the question could not be asked, and a
    /// file must not be taken for deleted because the link went quiet.
    pub async fn stamp(&self, path: &str) -> Result<Option<Stamp>> {
        match self.sftp.metadata(path).await {
            Ok(attrs) if attrs.is_dir() => Ok(None),
            Ok(attrs) => Ok(stamp_of(&attrs)),
            Err(e) if missing(&e) => Ok(None),
            Err(e) => Err(refuse(format!("could not check {path}: {e}"))),
        }
    }

    pub async fn read(&self, path: &str) -> Result<Document> {
        let describe = |e: &dyn std::fmt::Display| refuse(format!("could not open {path}: {e}"));

        let bytes = self.sftp.read(path).await.map_err(|e| describe(&e))?;
        let (text, eol, bom) = document::decode(bytes).map_err(|e| describe(&e))?;
        Ok(Document {
            path: path.to_owned(),
            name: name_of(path).to_owned(),
            text,
            eol,
            bom,
            stamp: self.stamp(path).await.ok().flatten(),
        })
    }

    /// Write a file, returning its stamp as written.
    pub async fn write(
        &self,
        path: &str,
        text: &str,
        eol: Eol,
        bom: bool,
    ) -> Result<Option<Stamp>> {
        let describe = |e: &dyn std::fmt::Display| refuse(format!("could not save {path}: {e}"));

        // In place, rather than to a neighbour that is then renamed over it:
        // that would save the text and lose the file's owner and its mode.
        let mut file = self.sftp.create(path).await.map_err(|e| describe(&e))?;
        file.write_all(&document::encode(text, eol, bom))
            .await
            .map_err(|e| describe(&e))?;
        file.shutdown().await.map_err(|e| describe(&e))?;
        self.stamp(path).await
    }

    /// The children of `dir`, less anything whose name matches `exclude`.
    pub async fn list(&self, dir: &str, exclude: &[String]) -> Result<Vec<Entry>> {
        let read = self
            .sftp
            .read_dir(dir)
            .await
            .map_err(|e| refuse(format!("could not read {dir}: {e}")))?;

        let mut entries = Vec::new();
        for entry in read {
            let name = entry.file_name();
            if name == "." || name == ".." || exclude.iter().any(|p| tree::matches(p, &name)) {
                continue;
            }
            let path = join(dir, &name);
            let attrs = entry.metadata();
            // Through the link, so one that points at a folder unfolds like
            // one. Only links are asked about: it is a request each.
            let dir = if attrs.is_symlink() {
                self.sftp
                    .metadata(path.as_str())
                    .await
                    .is_ok_and(|target| target.is_dir())
            } else {
                attrs.is_dir()
            };
            entries.push(Entry { name, path, dir });
        }
        tree::sort(&mut entries);
        Ok(entries)
    }

    /// When a folder's own contents last changed; `None` if it has gone. An
    /// error, as for [`Remote::stamp`], is not having been able to ask.
    pub async fn dir_stamp(&self, dir: &str) -> Result<Option<u64>> {
        match self.sftp.metadata(dir).await {
            Ok(attrs) if !attrs.is_dir() => Ok(None),
            Ok(attrs) => Ok(attrs.mtime.map(|seconds| u64::from(seconds) * 1000)),
            Err(e) if missing(&e) => Ok(None),
            Err(e) => Err(refuse(format!("could not check {dir}: {e}"))),
        }
    }

    fn exists_error(path: &str) -> Error {
        refuse(format!("\"{}\" already exists.", name_of(path)))
    }

    /// Create an empty file called `name` in `dir`.
    pub async fn create_file(&self, dir: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let path = join(dir, name);
        // Refused by the server if it is there, so nothing that appears
        // between a check and the create gets emptied.
        let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
        match self.sftp.open_with_flags(path.as_str(), flags).await {
            Ok(mut file) => {
                let _ = file.shutdown().await;
                Ok(path)
            }
            Err(_) if self.exists(&path).await => Err(Self::exists_error(&path)),
            Err(e) => Err(refuse(format!("could not create {path}: {e}"))),
        }
    }

    /// Create a folder called `name` in `dir`.
    pub async fn create_dir(&self, dir: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let path = join(dir, name);
        match self.sftp.create_dir(path.as_str()).await {
            Ok(()) => Ok(path),
            Err(_) if self.exists(&path).await => Err(Self::exists_error(&path)),
            Err(e) => Err(refuse(format!("could not create {path}: {e}"))),
        }
    }

    /// Give a file or folder a new name, in the folder it is already in.
    pub async fn rename(&self, path: &str, name: &str) -> Result<String> {
        validate_name(name)?;
        let target = join(parent_of(path), name);
        // A rename over an existing file replaces it on some servers.
        if self.exists(&target).await {
            return Err(Self::exists_error(&target));
        }
        self.sftp
            .rename(path, target.as_str())
            .await
            .map_err(|e| refuse(format!("could not rename {path}: {e}")))?;
        Ok(target)
    }

    /// Delete a file, or a folder and everything in it. There is no bin on
    /// the other end: this is for good.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let describe = |e: SftpError| refuse(format!("could not delete {path}: {e}"));

        // Not followed: deleting a link removes the link, never its target.
        let attrs = self.sftp.symlink_metadata(path).await.map_err(describe)?;
        if !attrs.is_dir() {
            return self.sftp.remove_file(path).await.map_err(describe);
        }

        // SFTP removes only empty folders, so the contents go first. Walked
        // with a list rather than by recursion: `emptied` is the folders in
        // the order found, which reversed is deepest first.
        let mut pending = vec![path.to_owned()];
        let mut emptied = Vec::new();
        while let Some(dir) = pending.pop() {
            for entry in self.sftp.read_dir(dir.as_str()).await.map_err(describe)? {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child = join(&dir, &name);
                if entry.metadata().is_dir() {
                    pending.push(child);
                } else {
                    self.sftp.remove_file(child).await.map_err(describe)?;
                }
            }
            emptied.push(dir);
        }
        for dir in emptied.into_iter().rev() {
            self.sftp.remove_dir(dir).await.map_err(describe)?;
        }
        Ok(())
    }

    async fn exists(&self, path: &str) -> bool {
        !matches!(self.sftp.symlink_metadata(path).await, Err(e) if missing(&e))
    }

    /// The state of the repository `dir` is in, as [`crate::git::status`]
    /// finds it here: `git`'s own count where the remote has `git`, and the
    /// branch alone where it has not.
    pub async fn git_status(&self, dir: &str) -> Option<crate::git::Status> {
        let script = format!(
            "cd {} 2>/dev/null && {} 2>/dev/null",
            quoted(dir),
            crate::git::STATUS
        );
        if let Ok(printed) = self.output(Self::login(&script)).await {
            if let Some(status) = crate::git::parse_status(&printed) {
                return Some(status);
            }
        }
        self.git_branch(dir)
            .await
            .map(crate::git::Status::of_branch)
    }

    /// The branch `dir` is on, as [`crate::git::branch`] finds it here: its
    /// own repository's, or the nearest one above it.
    pub async fn git_branch(&self, dir: &str) -> Option<String> {
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).ok();

        let mut folder = dir.trim_end_matches('/');
        for _ in 0..crate::git::MAX_DEPTH {
            let dot_git = format!("{folder}/.git");
            if let Ok(head) = self.sftp.read(format!("{dot_git}/HEAD")).await {
                return crate::git::head_label(&text(head)?);
            }
            // Not a folder with a `HEAD` in it; a worktree's pointer, perhaps.
            if let Ok(pointer) = self.sftp.read(dot_git.as_str()).await {
                let pointer = text(pointer)?;
                let gitdir = crate::git::gitdir_of(&pointer)?;
                let gitdir = match gitdir.starts_with('/') {
                    true => gitdir.to_owned(),
                    false => format!("{folder}/{gitdir}"),
                };
                let head = self.sftp.read(format!("{gitdir}/HEAD")).await.ok()?;
                return crate::git::head_label(&text(head)?);
            }
            if folder.is_empty() {
                break;
            }
            folder = folder.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
        None
    }

    // --- terminals ------------------------------------------------------------

    /// Start a shell on the remote, in `cwd`, as a channel on this connection.
    ///
    /// `on_exit` runs once, after the last of the output, when the shell has
    /// ended or the connection under it has.
    pub async fn shell<D, E>(
        &self,
        cwd: Option<&str>,
        cols: u16,
        rows: u16,
        mut on_data: D,
        on_exit: E,
    ) -> Result<RemoteTerminal>
    where
        D: FnMut(&[u8]) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        let describe = |what: &str, e: russh::Error| refuse(format!("{what}: {e}"));

        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| describe("the server would not open a terminal", e))?;
        // Usually refused, since a server only takes the variables it lists,
        // and one packet where it is not.
        let _ = channel.set_env(false, "COLORTERM", "truecolor").await;
        channel
            .request_pty(
                true,
                "xterm-256color",
                u32::from(cols.max(1)),
                u32::from(rows.max(1)),
                0,
                0,
                &[],
            )
            .await
            .map_err(|e| describe("the server refused a terminal", e))?;

        match cwd {
            // The user's own login shell, started where they are working. A
            // folder that has gone is not a reason to have no shell at all.
            Some(dir) => {
                let command = format!(
                    "cd {} 2>/dev/null; exec \"${{SHELL:-/bin/sh}}\" -l",
                    quoted(dir)
                );
                channel.exec(true, command).await
            }
            None => channel.request_shell(true).await,
        }
        .map_err(|e| describe("the server refused a shell", e))?;

        // Two tasks, one for each way: the output is not read while the screen
        // is behind (see `Flow`), and what is typed -- a Ctrl+C, above all --
        // must not have to wait for that.
        let (mut output, input) = channel.split();
        let (commands, mut queued) = unbounded_channel::<Command>();
        tokio::spawn(async move {
            while let Some(command) = queued.recv().await {
                match command {
                    Command::Data(bytes) => {
                        if input.data(&bytes[..]).await.is_err() {
                            break;
                        }
                    }
                    Command::Resize { cols, rows } => {
                        let _ = input
                            .window_change(u32::from(cols), u32::from(rows), 0, 0)
                            .await;
                    }
                    // The shell ended, or the terminal was dropped.
                    Command::Close => break,
                }
            }
            let _ = input.close().await;
        });

        // While this is not reading, the connection's own queue for the
        // channel fills, and then the connection waits too: that is what
        // passes the wait back to the program, and it holds up the window's
        // files for as long. The screen catches up in a fraction of a second.
        let flow = Arc::new(Flow::default());
        tokio::spawn({
            let (flow, commands) = (Arc::clone(&flow), commands.clone());
            async move {
                let mut batch = Vec::new();
                let mut sent: Option<Instant> = None;
                let mut open = true;
                while open {
                    flow.ready().await;
                    match output.wait().await {
                        // The shell's stderr belongs on screen with the rest.
                        Some(ChannelMsg::Data { data })
                        | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            batch.extend_from_slice(&data)
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => continue,
                    }
                    // What comes in the rest of the frame goes with it. After
                    // a quiet spell there is no rest, and it goes at once.
                    let until = sent.map_or_else(Instant::now, |at| at + FRAME);
                    while batch.len() < BATCH {
                        match tokio::time::timeout_at(until, output.wait()).await {
                            Ok(Some(ChannelMsg::Data { data }))
                            | Ok(Some(ChannelMsg::ExtendedData { data, .. })) => {
                                batch.extend_from_slice(&data)
                            }
                            Ok(Some(ChannelMsg::Eof)) | Ok(Some(ChannelMsg::Close)) | Ok(None) => {
                                open = false;
                                break;
                            }
                            Ok(Some(_)) => {}
                            Err(_) => break,
                        }
                    }
                    flow.sent(batch.len());
                    on_data(&batch);
                    batch.clear();
                    sent = Some(Instant::now());
                }
                let _ = commands.send(Command::Close);
                on_exit();
            }
        });

        Ok(RemoteTerminal { commands, flow })
    }

    // --- what is running there ---------------------------------------------------

    /// Send a signal to something run in a shell on the remote, and to
    /// everything it has started: `INT` as Ctrl+C would, `TERM` to ask it to
    /// end, `KILL` to end it.
    ///
    /// A shell that keeps track of what it runs gives each thing its own
    /// group, and the group is what is signalled, so that a build goes with
    /// its compilers. `shell` is the shell it was run from: should the two
    /// share a group, the signal goes to the one process and not to the shell.
    pub async fn signal(&self, pid: u32, shell: u32, signal: &str) -> Result<()> {
        if !matches!(signal, "INT" | "TERM" | "KILL") {
            return Err(refuse(format!(
                "{signal} is not a signal that is sent from here."
            )));
        }
        let script = format!(
            "g=$(ps -o pgid= -p {pid} 2>/dev/null | tr -d ' '); \
             s=$(ps -o pgid= -p {shell} 2>/dev/null | tr -d ' '); \
             if [ -n \"$g\" ] && [ \"$g\" != \"$s\" ]; then kill -s {signal} -- \"-$g\"; \
             else kill -s {signal} {pid}; fi 2>&1"
        );
        let said = self
            .output(Self::login(&format!("exec sh -c {}", quoted(&script))))
            .await?;
        // Nothing said is nothing wrong; what `kill` does say is why not.
        match said.lines().rev().find(|line| line.contains("kill")) {
            Some(why) => Err(refuse(why.trim().to_owned())),
            None => Ok(()),
        }
    }

    // --- how busy it is -----------------------------------------------------------

    /// Keep reading how busy the remote is, as a channel on this connection.
    /// `on_sample` hears every couple of seconds, for as long as what comes
    /// back is kept; a machine there is no reading of is never heard from.
    pub async fn watch_system<S>(&self, mut on_sample: S) -> Result<RemoteStats>
    where
        S: FnMut(sysinfo::Sample) + Send + 'static,
    {
        let describe = |what: &str, e: russh::Error| refuse(format!("{what}: {e}"));

        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| describe("the server would not open a channel", e))?;
        // By way of the login shell, for its `PATH`, and then `sh` whatever
        // that shell is: the script is not one a fish would read.
        let command = Self::login(&format!("exec sh -c {}", quoted(sysinfo::SCRIPT)));
        channel
            .exec(true, command)
            .await
            .map_err(|e| describe("the server refused to run it", e))?;

        let (commands, mut queued) = unbounded_channel::<Command>();
        tokio::spawn(async move {
            let mut sampler = sysinfo::Sampler::default();
            loop {
                tokio::select! {
                    command = queued.recv() => match command {
                        Some(Command::Close) | None => break,
                        Some(_) => {}
                    },
                    message = channel.wait() => match message {
                        Some(ChannelMsg::Data { data }) => {
                            sampler.feed(&data).into_iter().for_each(&mut on_sample);
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    },
                }
            }
            // Closing it is what ends the script: it has no one to print to.
            let _ = channel.close().await;
        });

        Ok(RemoteStats { commands })
    }

    // --- tmux -------------------------------------------------------------------

    /// The tmux sessions there are on the remote, and whether it has tmux
    /// at all. Asked without attaching to anything: with no session to
    /// attach to, there is no other way to ask.
    pub async fn tmux_sessions(&self) -> Result<tmux::Listing> {
        /// Marks an answer, among whatever else a login shell prints.
        const MISSING: &str = "thread-tmux:missing";
        const SESSION: &str = "thread-tmux:";
        // No server running is no sessions, and tmux says so as an error.
        let script = format!(
            "command -v tmux >/dev/null 2>&1 || {{ echo {MISSING}; exit 0; }}; \
             tmux list-sessions -F '{SESSION}{}' 2>/dev/null",
            tmux::SESSIONS_FORMAT
        );
        let printed = self.output(Self::login(&script)).await?;
        let lines = || printed.lines().map(|line| line.trim_end_matches('\r'));
        Ok(tmux::Listing {
            installed: !lines().any(|line| line.trim() == MISSING),
            sessions: lines()
                .filter_map(|line| tmux::parse_session(line.strip_prefix(SESSION)?))
                .collect(),
        })
    }

    /// Attach to tmux on the remote, in its control mode, as a channel on
    /// this connection. `on_event` hears what there is to show; `on_exit`
    /// hears once, when tmux has let go -- detached, its last session ended,
    /// or the connection gone from under it. The sessions themselves stay.
    pub async fn tmux<V, E>(
        &self,
        start: tmux::Start,
        cols: u16,
        rows: u16,
        history: u32,
        mut on_event: V,
        on_exit: E,
    ) -> Result<RemoteTmux>
    where
        V: FnMut(tmux::Event) + Send + 'static,
        E: FnOnce(tmux::Ended) + Send + 'static,
    {
        let describe = |what: &str, e: russh::Error| refuse(format!("{what}: {e}"));

        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| describe("the server would not open a channel", e))?;
        // No terminal: control mode is lines of text, and wants none.
        let command = Self::login(&format!("exec tmux {}", start.arguments(cols, rows)));
        channel
            .exec(true, command)
            .await
            .map_err(|e| describe("the server refused to run tmux", e))?;

        let (mut output, input) = channel.split();
        let (commands, mut queued) = unbounded_channel::<Command>();
        tokio::spawn(async move {
            while let Some(command) = queued.recv().await {
                match command {
                    Command::Data(bytes) => {
                        if input.data(&bytes[..]).await.is_err() {
                            break;
                        }
                    }
                    Command::Resize { .. } => {}
                    Command::Close => break,
                }
            }
            // Its input ending is what tells tmux to detach.
            let _ = input.close().await;
        });

        let engine = Arc::new(Mutex::new(tmux::Engine::new(cols, rows, history)));
        // One count for every pane: they share the channel, so what holds
        // one back holds them all. See `shell` for the rest of it.
        let flow = Arc::new(Flow::default());
        tokio::spawn({
            let (engine, flow, commands) =
                (Arc::clone(&engine), Arc::clone(&flow), commands.clone());
            async move {
                let mut batch = Vec::new();
                // What tmux, or the shell that could not find it, said aside.
                let mut aside = Vec::new();
                let mut sent: Option<Instant> = None;
                let mut open = true;
                while open {
                    flow.ready().await;
                    match output.wait().await {
                        Some(ChannelMsg::Data { data }) => batch.extend_from_slice(&data),
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            aside.extend_from_slice(&data);
                            continue;
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => continue,
                    }
                    let until = sent.map_or_else(Instant::now, |at| at + FRAME);
                    while batch.len() < BATCH {
                        match tokio::time::timeout_at(until, output.wait()).await {
                            Ok(Some(ChannelMsg::Data { data })) => batch.extend_from_slice(&data),
                            Ok(Some(ChannelMsg::ExtendedData { data, .. })) => {
                                aside.extend_from_slice(&data)
                            }
                            Ok(Some(ChannelMsg::Eof)) | Ok(Some(ChannelMsg::Close)) | Ok(None) => {
                                open = false;
                                break;
                            }
                            Ok(Some(_)) => {}
                            Err(_) => break,
                        }
                    }

                    let mut out = tmux::Out::default();
                    if let Ok(mut engine) = engine.lock() {
                        engine.feed(&batch, &mut out);
                    }
                    batch.clear();
                    if !out.commands.is_empty() {
                        let _ = commands.send(Command::Data(out.commands));
                    }
                    for event in out.events {
                        if let tmux::Event::Output { data, .. } = &event {
                            flow.sent(data.len());
                        }
                        on_event(event);
                    }
                    sent = Some(Instant::now());
                }
                let _ = commands.send(Command::Close);

                let aside = String::from_utf8_lossy(&aside).trim().to_owned();
                let ended = match engine.lock() {
                    Ok(engine) => tmux::Ended {
                        attached: engine.started(),
                        reason: engine
                            .reason()
                            .map(str::to_owned)
                            .or((!aside.is_empty()).then_some(aside)),
                    },
                    Err(_) => tmux::Ended {
                        attached: false,
                        reason: None,
                    },
                };
                on_exit(ended);
            }
        });

        Ok(RemoteTmux {
            commands,
            flow,
            engine,
        })
    }

    // --- language servers -------------------------------------------------------

    /// A command for the remote that runs `script` in the user's login shell,
    /// which is where their `PATH` is whole: a server installed for the user
    /// (a Nix profile, `~/.local/bin`, npm's own folder) is not on the bare
    /// one a command over SSH is otherwise given.
    fn login(script: &str) -> String {
        format!("exec \"${{SHELL:-/bin/sh}}\" -lc {}", quoted(script))
    }

    /// Run a command on the remote and give back what it printed.
    async fn output(&self, command: String) -> Result<String> {
        let run = async {
            let mut channel = self.handle.channel_open_session().await?;
            channel.exec(true, command).await?;
            let mut printed = Vec::new();
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => printed.extend_from_slice(&data),
                    ChannelMsg::Eof | ChannelMsg::Close => break,
                    _ => {}
                }
            }
            let _ = channel.close().await;
            Ok::<_, russh::Error>(printed)
        };
        match tokio::time::timeout(Duration::from_secs(15), run).await {
            Ok(Ok(printed)) => Ok(String::from_utf8_lossy(&printed).into_owned()),
            Ok(Err(e)) => Err(refuse(format!("the server would not run a command: {e}"))),
            Err(_) => Err(refuse("the remote took too long to answer")),
        }
    }

    /// The language servers Thread knows of, and which of them are installed
    /// on the remote: found by its login shell, as they would be if typed.
    pub async fn servers(&self) -> Vec<crate::lsp::Info> {
        /// Marks an answer, among whatever else a login shell prints.
        const FOUND: &str = "thread-found:";
        let names: Vec<&str> = crate::lsp::CATALOG
            .iter()
            .map(|spec| spec.command)
            .collect();
        let script = format!(
            "for c in {}; do command -v \"$c\" >/dev/null 2>&1 && echo \"{FOUND}$c\"; done",
            names.join(" ")
        );
        let printed = self.output(Self::login(&script)).await.unwrap_or_default();
        let found: Vec<&str> = printed
            .lines()
            .filter_map(|line| line.trim().strip_prefix(FOUND))
            .collect();

        crate::lsp::CATALOG
            .iter()
            .map(|spec| crate::lsp::Info::of(spec, found.contains(&spec.command)))
            .collect()
    }

    /// Start a language server on the remote, in `cwd`, as a channel on this
    /// connection. `on_message` hears each message it sends, whole; `on_exit`
    /// hears once, when it has ended or the connection under it has.
    pub async fn serve<M, E>(
        &self,
        id: &str,
        cwd: Option<&str>,
        mut on_message: M,
        on_exit: E,
    ) -> Result<RemoteServer>
    where
        M: FnMut(&str) + Send + 'static,
        E: FnOnce() + Send + 'static,
    {
        let spec = crate::lsp::spec(id)?;
        let describe = |what: &str, e: russh::Error| refuse(format!("{what}: {e}"));

        let mut line = String::from("exec ");
        line.push_str(spec.command);
        for arg in spec.args {
            line.push(' ');
            line.push_str(&quoted(arg));
        }
        let command = match cwd {
            Some(dir) => format!("cd {} 2>/dev/null; {}", quoted(dir), Self::login(&line)),
            None => Self::login(&line),
        };

        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|e| describe("the server would not open a channel", e))?;
        // No terminal: what goes over this is the protocol, byte for byte.
        channel
            .exec(true, command)
            .await
            .map_err(|e| describe("the server refused to run it", e))?;

        let name = spec.id;
        let (commands, mut queued) = unbounded_channel::<Command>();
        tokio::spawn(async move {
            let mut framer = crate::lsp::Framer::default();
            loop {
                tokio::select! {
                    command = queued.recv() => match command {
                        Some(Command::Data(bytes)) => {
                            if channel.data(&bytes[..]).await.is_err() {
                                break;
                            }
                        }
                        Some(Command::Resize { .. }) => {}
                        Some(Command::Close) | None => break,
                    },
                    message = channel.wait() => match message {
                        Some(ChannelMsg::Data { data }) => {
                            for message in framer.push(&data) {
                                on_message(&message);
                            }
                        }
                        // What it says to the side: "command not found", too.
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            tracing::debug!(
                                target: "thread::lsp",
                                "{name}: {}",
                                String::from_utf8_lossy(&data).trim_end()
                            );
                        }
                        Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                        Some(_) => {}
                    },
                }
            }
            let _ = channel.close().await;
            on_exit();
        });

        Ok(RemoteServer { commands })
    }
}

/// A language server on the remote. Dropping it closes its channel, which
/// ends it.
pub struct RemoteServer {
    commands: UnboundedSender<Command>,
}

impl crate::lsp::Serving for RemoteServer {
    fn send(&self, message: &str) {
        let _ = self
            .commands
            .send(Command::Data(crate::lsp::frame(message)));
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}

/// Prove who we are, by whichever way the target says. `Ok(false)` is the
/// server saying no; an error is not getting as far as asking.
async fn authenticate(
    handle: &mut client::Handle<VerifyHostKey>,
    target: &Target,
) -> std::result::Result<bool, ConnectError> {
    let failed =
        |e: russh::Error| ConnectError::new(Failure::Auth, format!("Signing in failed: {e}"));

    let result = match &target.auth {
        Auth::Password { password } => handle
            .authenticate_password(target.username.clone(), password.clone())
            .await
            .map_err(failed)?,

        Auth::Key { path, passphrase } => {
            let passphrase = (!passphrase.is_empty()).then_some(passphrase.as_str());
            // Not being able to read the key is a different thing from the
            // server turning it down, and should say so.
            let key = russh::keys::load_secret_key(path, passphrase).map_err(|e| {
                let hint = match passphrase {
                    None => "If the key is encrypted, enter its passphrase.",
                    Some(_) => "Check the passphrase.",
                };
                ConnectError::new(
                    Failure::Auth,
                    format!("Could not read the private key {path}: {e}. {hint}"),
                )
            })?;
            // The server's pick of hash: modern ones refuse RSA with SHA-1.
            let hash = handle
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten();
            handle
                .authenticate_publickey(
                    target.username.clone(),
                    russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                )
                .await
                .map_err(failed)?
        }
    };
    Ok(result.success())
}

enum Command {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Close,
}

/// A shell on the remote. Dropping it closes its channel, which ends it.
pub struct RemoteTerminal {
    commands: UnboundedSender<Command>,
    flow: Arc<Flow>,
}

/// The most output handed over as one piece, give or take a packet.
const BATCH: usize = 64 * 1024;

impl Terminal for RemoteTerminal {
    fn write(&self, data: &[u8]) {
        let _ = self.commands.send(Command::Data(data.to_vec()));
    }

    fn resize(&self, cols: u16, rows: u16) {
        let _ = self.commands.send(Command::Resize { cols, rows });
    }

    fn flow(&self) -> &Flow {
        &self.flow
    }
}

impl Drop for RemoteTerminal {
    fn drop(&mut self) {
        // Its tab has gone, so nothing is left to say the output was drawn.
        self.flow.release();
        let _ = self.commands.send(Command::Close);
    }
}

/// A watch kept on how busy the remote is. Dropping it ends the watch.
pub struct RemoteStats {
    commands: UnboundedSender<Command>,
}

impl Drop for RemoteStats {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}

/// An attachment to tmux on the remote. Dropping it detaches: the sessions,
/// and everything running in them, carry on.
pub struct RemoteTmux {
    commands: UnboundedSender<Command>,
    flow: Arc<Flow>,
    engine: Arc<Mutex<tmux::Engine>>,
}

impl RemoteTmux {
    /// Do something the page asked for. Returns once it is queued.
    pub fn act(&self, action: tmux::Action) {
        let mut out = tmux::Out::default();
        if let Ok(mut engine) = self.engine.lock() {
            engine.act(action, &mut out);
        }
        if !out.commands.is_empty() {
            let _ = self.commands.send(Command::Data(out.commands));
        }
    }

    /// How much of what the panes printed is still on its way to the screen.
    pub fn flow(&self) -> &Flow {
        &self.flow
    }
}

impl Drop for RemoteTmux {
    fn drop(&mut self) {
        self.flow.release();
        let _ = self.commands.send(Command::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_paths_are_taken_apart_on_forward_slashes() {
        assert_eq!(name_of("/home/ash/notes.txt"), "notes.txt");
        assert_eq!(name_of("/home/ash/"), "ash");
        assert_eq!(parent_of("/home/ash/notes.txt"), "/home/ash");
        assert_eq!(parent_of("/notes.txt"), "/");
        assert_eq!(join("/home/ash", "x"), "/home/ash/x");
        assert_eq!(join("/", "etc"), "/etc");
    }

    /// A backslash and a colon are ordinary characters over there.
    #[test]
    fn a_name_is_one_component_and_little_else_is_asked_of_it() {
        assert!(validate_name("notes.txt").is_ok());
        assert!(validate_name("a:b\\c").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("a/b").is_err());
    }

    #[test]
    fn a_path_survives_the_remote_shell() {
        assert_eq!(quoted("/srv/my app"), "'/srv/my app'");
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }

    #[test]
    fn a_label_names_the_port_only_when_it_is_unusual() {
        assert_eq!(label("ash", "box", 22), "ash@box");
        assert_eq!(label("ash", "box", 2222), "ash@box:2222");
    }

    /// Nothing is listening on port 1; this has to fail fast and say where.
    #[tokio::test]
    async fn a_host_that_refuses_is_unreachable_not_a_bad_password() {
        let target = Target {
            host: "127.0.0.1".into(),
            port: 1,
            username: "ash".into(),
            auth: Auth::Password {
                password: "x".into(),
            },
            trust_new_key: false,
        };
        let error = Remote::connect(&target).await.err().expect("nothing there");
        assert_eq!(error.kind, Failure::Unreachable);
        assert!(error.message.contains("127.0.0.1:1"), "got {error}");
    }
}
