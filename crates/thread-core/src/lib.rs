//! Core for Thread.
//!
//! Everything here is UI-agnostic: the config, the stores the window chrome
//! reads and writes, the file handling behind the editor and the tree, and
//! the terminal's shell.

use std::fmt;
use std::path::PathBuf;

pub mod config;
pub mod connections;
pub mod document;
pub mod fsops;
pub mod git;
pub mod known_hosts;
pub mod pins;
pub mod remote;
pub mod session;
pub mod terminal;
pub mod tree;
pub mod vault;

pub use config::{Appearance, Config, Material};
pub use document::{Document, Eol, Stamp};
pub use pins::{Pin, Store as PinStore};
pub use session::Session;
pub use tree::Entry;

/// Errors surfaced by the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// `%APPDATA%\thread`, created if missing.
///
/// `THREAD_DATA_DIR` overrides it, so tests never touch the real config.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("THREAD_DATA_DIR") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        return Ok(dir);
    }

    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Other(anyhow::anyhow!("APPDATA is not set")))?;

    let dir = base.join("thread");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Drop a UTF-8 byte-order mark, if the file has one.
///
/// Every store here is a JSON file a user might reasonably open and edit, and
/// the Windows tools they would reach for -- Notepad, `Out-File`, `Set-Content`
/// -- write UTF-8 *with* a BOM by default. `serde_json` treats those three
/// leading bytes as a parse error at line 1 column 1, so without this a
/// hand-edited config comes back as "corrupt" with nothing visibly wrong in it.
pub fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Build/version banner.
pub struct Version;

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "thread-core {}", env!("CARGO_PKG_VERSION"))
    }
}
