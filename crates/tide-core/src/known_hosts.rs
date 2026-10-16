//! The host keys that have been trusted, in `%APPDATA%\tide\known_hosts.json`.
//!
//! Trust on first use. Accepting whatever key a server presents is merely
//! sloppy while a password is typed by hand; it becomes dangerous the moment
//! one is *saved*, because the app would then hand it to whatever answers at
//! that address.
//!
//! * **Unknown**: first sight of this host. Ask, then remember.
//! * **Match**: connect without a word.
//! * **Changed**: refuse. The server was rebuilt, or something is pretending
//!   to be it, and only a person can say which.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "known_hosts.json";

/// Held while the file is read or rewritten. Two connections being made at
/// once -- two windows starting together -- would otherwise each read it,
/// add a host, and write it back, and the second would write the first's
/// host out of it; or one would read it half written and take it for empty.
static FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What was known about a host before this connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Unknown,
    Match,
    Changed { expected: String },
}

/// `host` or `[host]:port`, to its key's fingerprint (`SHA256:...`).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KnownHosts {
    #[serde(default)]
    hosts: BTreeMap<String, String>,
}

/// As OpenSSH writes it: only a port that is not the default is spelled out.
fn key_for(host: &str, port: u16) -> String {
    let host = host.to_ascii_lowercase();
    if port == 22 {
        host
    } else {
        format!("[{host}]:{port}")
    }
}

impl KnownHosts {
    pub fn load() -> Result<Self> {
        let _held = FILE_LOCK.lock();
        Self::read()
    }

    /// Trust a fingerprint from now on, and write it down: added to what is
    /// in the file at this moment, not to what was read from it earlier.
    pub fn remember(host: &str, port: u16, fingerprint: &str) -> Result<()> {
        let _held = FILE_LOCK.lock();
        let mut known = Self::read().unwrap_or_default();
        known.trust(host, port, fingerprint);
        known.save()
    }

    fn read() -> Result<Self> {
        let path = data_dir()?.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(strip_bom(&text)).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} cannot be read ({e}); move it aside to start afresh",
                    path.display()
                ))
            }),
            // No file yet means nothing has been trusted.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not write the known hosts: {e}")))?;
        std::fs::write(data_dir()?.join(FILE), text)?;
        Ok(())
    }

    pub fn verdict(&self, host: &str, port: u16, fingerprint: &str) -> Verdict {
        match self.hosts.get(&key_for(host, port)) {
            None => Verdict::Unknown,
            Some(known) if known == fingerprint => Verdict::Match,
            Some(known) => Verdict::Changed {
                expected: known.clone(),
            },
        }
    }

    /// Trust this fingerprint from now on, in place of any before it.
    pub fn trust(&mut self, host: &str, port: u16, fingerprint: &str) {
        self.hosts
            .insert(key_for(host, port), fingerprint.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_unknown_then_matches_then_is_noticed_changing() {
        let mut known = KnownHosts::default();
        assert_eq!(known.verdict("box", 22, "SHA256:a"), Verdict::Unknown);

        known.trust("box", 22, "SHA256:a");
        assert_eq!(known.verdict("box", 22, "SHA256:a"), Verdict::Match);
        assert_eq!(known.verdict("BOX", 22, "SHA256:a"), Verdict::Match);
        assert_eq!(
            known.verdict("box", 22, "SHA256:b"),
            Verdict::Changed {
                expected: "SHA256:a".into()
            }
        );
    }

    /// Two servers on one address are two hosts.
    #[test]
    fn another_port_is_another_host() {
        let mut known = KnownHosts::default();
        known.trust("box", 22, "SHA256:a");
        assert_eq!(known.verdict("box", 2222, "SHA256:a"), Verdict::Unknown);
    }
}
