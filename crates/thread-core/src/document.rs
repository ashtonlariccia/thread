//! Reading and writing the files being edited.
//!
//! The editor works on `\n`-separated text and nothing else. What a file had
//! on disk beyond that — its line ending and whether it led with a byte-order
//! mark — is carried alongside the text and put back on save, so opening a
//! file and saving it unchanged leaves it byte-identical.
//!
//! A [`Stamp`] is taken whenever a file is read or written, so the editor can
//! later tell that something else has changed it on disk.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const BOM: &str = "\u{feff}";

/// The line ending a file is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Eol {
    #[default]
    Lf,
    Crlf,
}

/// What a file looked like on disk at one moment: enough to notice that it
/// has been written since, without reading it.
///
/// Modification time alone is not trusted — some tools restore it — so the
/// length is compared too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    /// Milliseconds since the Unix epoch.
    pub modified: u64,
    pub len: u64,
}

/// The file's current stamp, or `None` if it cannot be read — which for an
/// open file almost always means it has been deleted or moved.
pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(Stamp {
        modified: modified.as_millis() as u64,
        len: meta.len(),
    })
}

/// A file as the editor sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub path: String,
    /// The file name alone, for display.
    pub name: String,
    /// Always `\n`-separated, whatever the file used.
    pub text: String,
    pub eol: Eol,
    pub bom: bool,
    /// The file as it was when this was read.
    pub stamp: Option<Stamp>,
}

/// Split a file's bytes into editor text and what must be restored on save.
///
/// A file with any `\r\n` in it is treated as a CRLF file. One with mixed
/// endings therefore comes back out uniformly CRLF, which is the repair an
/// editor is expected to make rather than a loss.
pub fn decode(bytes: Vec<u8>) -> Result<(String, Eol, bool)> {
    let text =
        String::from_utf8(bytes).map_err(|_| Error::Other(anyhow::anyhow!("not UTF-8 text")))?;

    let (text, bom) = match text.strip_prefix(BOM) {
        Some(rest) => (rest, true),
        None => (text.as_str(), false),
    };

    if text.contains("\r\n") {
        Ok((text.replace("\r\n", "\n"), Eol::Crlf, bom))
    } else {
        Ok((text.to_owned(), Eol::Lf, bom))
    }
}

/// The reverse of [`decode`].
pub fn encode(text: &str, eol: Eol, bom: bool) -> Vec<u8> {
    let mut out = String::with_capacity(text.len() + BOM.len());
    if bom {
        out.push_str(BOM);
    }
    match eol {
        Eol::Lf => out.push_str(text),
        Eol::Crlf => out.push_str(&text.replace('\n', "\r\n")),
    }
    out.into_bytes()
}

fn describe(path: &Path, action: &str, e: impl std::fmt::Display) -> Error {
    Error::Other(anyhow::anyhow!(
        "could not {action} {}: {e}",
        path.display()
    ))
}

pub fn read(path: &Path) -> Result<Document> {
    let bytes = std::fs::read(path).map_err(|e| describe(path, "open", e))?;
    let (text, eol, bom) = decode(bytes).map_err(|e| describe(path, "open", e))?;

    Ok(Document {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        text,
        eol,
        bom,
        stamp: stamp(path),
    })
}

/// Write a file, returning its stamp as written.
pub fn write(path: &Path, text: &str, eol: Eol, bom: bool) -> Result<Option<Stamp>> {
    std::fs::write(path, encode(text, eol, bom)).map_err(|e| describe(path, "save", e))?;
    Ok(stamp(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(original: &str) {
        let (text, eol, bom) = decode(original.as_bytes().to_vec()).unwrap();
        assert!(!text.contains('\r'), "the editor must only ever see \\n");
        assert_eq!(encode(&text, eol, bom), original.as_bytes());
    }

    #[test]
    fn an_unchanged_file_saves_byte_identical() {
        round_trip("one\ntwo\n");
        round_trip("one\r\ntwo\r\n");
        round_trip("\u{feff}one\r\ntwo");
        round_trip("\u{feff}one\ntwo");
        round_trip("");
    }

    #[test]
    fn line_endings_are_detected() {
        assert_eq!(decode(b"a\nb".to_vec()).unwrap().1, Eol::Lf);
        assert_eq!(decode(b"a\r\nb".to_vec()).unwrap().1, Eol::Crlf);
    }

    #[test]
    fn mixed_endings_come_out_uniform() {
        let (text, eol, bom) = decode(b"a\r\nb\nc".to_vec()).unwrap();
        assert_eq!(text, "a\nb\nc");
        assert_eq!(encode(&text, eol, bom), b"a\r\nb\r\nc");
    }

    #[test]
    fn a_byte_order_mark_is_kept_out_of_the_text() {
        let (text, _, bom) = decode("\u{feff}hi".as_bytes().to_vec()).unwrap();
        assert_eq!(text, "hi");
        assert!(bom);
    }

    /// Opening a binary as text and saving it would corrupt it.
    #[test]
    fn a_file_that_is_not_utf8_is_refused() {
        assert!(decode(vec![0xff, 0xfe, 0x00, 0x41]).is_err());
    }

    #[test]
    fn a_file_reads_and_writes_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "a\r\nb").unwrap();

        let doc = read(&path).unwrap();
        assert_eq!(doc.name, "note.txt");
        assert_eq!(doc.text, "a\nb");
        assert_eq!(doc.eol, Eol::Crlf);

        write(&path, "a\nb\nc", doc.eol, doc.bom).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"a\r\nb\r\nc");
    }

    #[test]
    fn a_stamp_notices_a_file_being_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "short").unwrap();

        let opened = read(&path).unwrap().stamp;
        assert_eq!(opened, stamp(&path), "untouched, so unchanged");

        // A different length, so this does not depend on the clock's resolution.
        std::fs::write(&path, "rather longer").unwrap();
        assert_ne!(opened, stamp(&path));
    }

    #[test]
    fn saving_returns_the_stamp_the_file_now_has() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");

        let written = write(&path, "hello", Eol::Lf, false).unwrap();
        assert!(written.is_some());
        assert_eq!(written, stamp(&path));
    }

    #[test]
    fn a_deleted_file_has_no_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.txt");
        std::fs::write(&path, "x").unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(stamp(&path), None);
        assert_eq!(stamp(dir.path()), None, "a directory is not a file");
    }

    #[test]
    fn a_missing_file_says_which_one() {
        let err = read(Path::new("Z:/no/such/file.txt")).unwrap_err();
        assert!(err.to_string().contains("file.txt"), "got {err}");
    }
}
