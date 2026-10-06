//! Saved connections, in `%APPDATA%\thread\connections.json`.
//!
//! A connection that is saved can be made again without typing anything, and
//! is what a session comes back to. One that is not saved lives as long as
//! the window it was made in.
//!
//! Passwords and passphrases are sealed (see [`crate::vault`]) and never
//! leave this module in the clear except as the [`Auth`] a connection is made
//! with. What the window is shown is a [`SavedConnection`], which has none.

use serde::{Deserialize, Serialize};

use crate::remote::Auth;
use crate::vault::{seal, to_hex, unseal};
use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "connections.json";

/// A saved connection as the window sees it: no secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedConnection {
    /// Stable, and not derived from the address: a session refers to it.
    pub id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
}

impl SavedConnection {
    fn matches(&self, host: &str, port: u16, username: &str) -> bool {
        self.host.eq_ignore_ascii_case(host) && self.port == port && self.username == username
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    #[serde(flatten)]
    connection: SavedConnection,
    /// Sealed: the password, or the key's passphrase.
    secret: String,
    /// The private key, for an entry that signs in with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key_path: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    entries: Vec<Entry>,
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).expect("the OS has no randomness to give");
    to_hex(&bytes)
}

impl Store {
    pub fn load() -> Result<Self> {
        let path = data_dir()?.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(strip_bom(&text)).map_err(|e| {
                Error::Other(anyhow::anyhow!(
                    "{} cannot be read ({e}); move it aside to start afresh",
                    path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(e)),
        }
    }

    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Other(anyhow::anyhow!("could not write the connections: {e}")))?;
        std::fs::write(data_dir()?.join(FILE), text)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<SavedConnection> {
        self.entries.iter().map(|e| e.connection.clone()).collect()
    }

    pub fn find(&self, host: &str, port: u16, username: &str) -> Option<SavedConnection> {
        self.entries
            .iter()
            .map(|e| &e.connection)
            .find(|c| c.matches(host, port, username))
            .cloned()
    }

    /// Save a connection, or bring the saved one for the same user and host
    /// up to date — under the id it already has, so a session that names it
    /// still finds it.
    pub fn upsert(
        &mut self,
        host: &str,
        port: u16,
        username: &str,
        auth: &Auth,
    ) -> Result<SavedConnection> {
        let (key_path, secret) = match auth {
            Auth::Password { password } => (None, seal(password)?),
            Auth::Key { path, passphrase } => (Some(path.clone()), seal(passphrase)?),
        };

        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|e| e.connection.matches(host, port, username))
        {
            entry.secret = secret;
            entry.key_path = key_path;
            return Ok(entry.connection.clone());
        }

        let connection = SavedConnection {
            id: new_id(),
            host: host.to_owned(),
            port,
            username: username.to_owned(),
        };
        self.entries.push(Entry {
            connection: connection.clone(),
            secret,
            key_path,
        });
        Ok(connection)
    }

    /// Forget a connection. Returns whether there was one to forget.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.connection.id != id);
        self.entries.len() != before
    }

    /// A saved connection and what it signs in with, unsealed.
    pub fn credentials(&self, id: &str) -> Result<(SavedConnection, Auth)> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.connection.id == id)
            .ok_or_else(|| Error::Other(anyhow::anyhow!("that connection is no longer saved")))?;

        let secret = unseal(&entry.secret)?;
        let auth = match &entry.key_path {
            Some(path) => Auth::Key {
                path: path.clone(),
                passphrase: secret,
            },
            None => Auth::Password { password: secret },
        };
        Ok((entry.connection.clone(), auth))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password(text: &str) -> Auth {
        Auth::Password {
            password: text.into(),
        }
    }

    #[test]
    fn a_saved_connection_gives_its_credentials_back() {
        let mut store = Store::default();
        let saved = store
            .upsert("box", 22, "ash", &password("hunter2"))
            .unwrap();

        let (connection, auth) = store.credentials(&saved.id).unwrap();
        assert_eq!(connection, saved);
        assert!(matches!(auth, Auth::Password { password } if password == "hunter2"));
    }

    /// The file is what gets backed up, synced and pasted into bug reports.
    #[test]
    fn the_password_is_not_in_what_is_written() {
        let mut store = Store::default();
        store
            .upsert("box", 22, "ash", &password("hunter2"))
            .unwrap();
        assert!(!serde_json::to_string(&store).unwrap().contains("hunter2"));
    }

    #[test]
    fn saving_the_same_user_and_host_again_keeps_the_id() {
        let mut store = Store::default();
        let first = store.upsert("box", 22, "ash", &password("old")).unwrap();
        let again = store.upsert("BOX", 22, "ash", &password("new")).unwrap();

        assert_eq!(first.id, again.id);
        assert_eq!(store.list().len(), 1);
        let (_, auth) = store.credentials(&first.id).unwrap();
        assert!(matches!(auth, Auth::Password { password } if password == "new"));
    }

    #[test]
    fn a_key_is_saved_by_its_path_with_its_passphrase_sealed() {
        let mut store = Store::default();
        let auth = Auth::Key {
            path: r"C:\Users\ash\.ssh\id_ed25519".into(),
            passphrase: "open sesame".into(),
        };
        let saved = store.upsert("box", 2222, "ash", &auth).unwrap();

        assert!(!serde_json::to_string(&store).unwrap().contains("sesame"));
        let (_, back) = store.credentials(&saved.id).unwrap();
        assert!(matches!(back, Auth::Key { path, passphrase }
            if path.ends_with("id_ed25519") && passphrase == "open sesame"));
    }

    #[test]
    fn a_forgotten_connection_has_no_credentials() {
        let mut store = Store::default();
        let saved = store.upsert("box", 22, "ash", &password("x")).unwrap();
        assert!(store.remove(&saved.id));
        assert!(!store.remove(&saved.id));
        assert!(store.credentials(&saved.id).is_err());
    }
}
