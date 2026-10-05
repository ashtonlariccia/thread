//! Core for Thread.
//!
//! Everything here is UI-agnostic: the stores the window chrome reads and
//! writes, and where they live on disk. The editor engine lands here too.

use std::fmt;
use std::path::PathBuf;

pub mod document;
pub mod pins;
pub mod settings;

pub use document::{Document, Eol};
pub use pins::{Pin, Store as PinStore};
pub use settings::{Appearance, Material};

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
/// `THREAD_DATA_DIR` overrides it, so tests never touch the real settings.
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
