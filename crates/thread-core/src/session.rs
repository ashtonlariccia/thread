//! What was open when Thread was last closed, in `%APPDATA%\thread\session.json`.
//!
//! Written by the app as things are opened and closed, and read once at
//! launch. Unlike the config this is not a file anyone edits: it is state, not
//! preference, which is why it is JSON beside the config rather than in it.
//!
//! Nothing here is ever worth failing over. A session that cannot be read is
//! an empty one, and the app starts as it would the first time.

use serde::{Deserialize, Serialize};

use crate::{data_dir, strip_bom, Error, Result};

const FILE: &str = "session.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Session {
    /// The folders open in the file tree, in order.
    pub folders: Vec<String>,
    /// Every folder that was unfolded, the opened ones included.
    pub unfolded: Vec<String>,
    /// The files open in tabs, in order. Buffers never saved are not here:
    /// they have no path to come back to.
    pub files: Vec<String>,
    /// The file that was being edited.
    pub active: Option<String>,
    pub sidebar_collapsed: bool,
    pub sidebar_width: u32,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            unfolded: Vec::new(),
            files: Vec::new(),
            active: None,
            sidebar_collapsed: true,
            sidebar_width: 230,
        }
    }
}

/// The last session, or an empty one if there is none that can be read.
pub fn load() -> Session {
    data_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join(FILE)).ok())
        .and_then(|text| serde_json::from_str(strip_bom(&text)).ok())
        .unwrap_or_default()
}

pub fn save(session: &Session) -> Result<()> {
    let text = serde_json::to_string_pretty(session)
        .map_err(|e| Error::Other(anyhow::anyhow!("could not serialise the session: {e}")))?;
    std::fs::write(data_dir()?.join(FILE), text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_json() {
        let session = Session {
            folders: vec!["C:\\src\\thread".into()],
            unfolded: vec!["C:\\src\\thread".into(), "C:\\src\\thread\\ui".into()],
            files: vec!["C:\\src\\thread\\Cargo.toml".into()],
            active: Some("C:\\src\\thread\\Cargo.toml".into()),
            sidebar_collapsed: false,
            sidebar_width: 300,
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("sidebarCollapsed"), "got {json}");
        assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), session);
    }

    /// A session from an older build, or a hand-truncated one, still loads.
    #[test]
    fn missing_fields_take_their_defaults() {
        let session: Session = serde_json::from_str(r#"{"files": ["a.txt"]}"#).unwrap();
        assert_eq!(session.files, ["a.txt"]);
        assert!(session.folders.is_empty());
        assert!(session.sidebar_collapsed);
        assert_eq!(session.sidebar_width, 230);
    }
}
