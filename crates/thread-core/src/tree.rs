//! Listing a folder for the file tree.
//!
//! One directory at a time: the tree asks for a folder's children when it is
//! expanded, so opening a project never walks `node_modules` to draw a list
//! nobody has unfolded.

use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::{Error, Result};

/// One row of the tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub dir: bool,
}

/// Whether a file name matches an exclude pattern.
///
/// A glob over the *name*, not the path: `*` is any run of characters, `?` is
/// any one, and everything else is literal. Case-insensitive, as Windows file
/// names are. So `.*` is every dotfile and `*.log` every log, at any depth.
pub fn matches(pattern: &str, name: &str) -> bool {
    fn go(pattern: &[char], name: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| go(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && go(rest, &name[1..]),
            Some((c, rest)) => name.first() == Some(c) && go(rest, &name[1..]),
        }
    }

    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let name: Vec<char> = name.to_lowercase().chars().collect();
    go(&pattern, &name)
}

/// Folders first, then files, each in name order ignoring case — the order
/// every file explorer uses, so nothing has to be hunted for.
pub(crate) fn sort(entries: &mut [Entry]) {
    entries.sort_by_cached_key(|e| (!e.dir, e.name.to_lowercase()));
}

/// The children of `dir`, less anything whose name matches `exclude`.
pub fn list(dir: &Path, exclude: &[String]) -> Result<Vec<Entry>> {
    let read = std::fs::read_dir(dir)
        .map_err(|e| Error::Other(anyhow::anyhow!("could not read {}: {e}", dir.display())))?;

    let mut entries: Vec<Entry> = read
        // An entry that vanished or cannot be read mid-listing is skipped
        // rather than failing the whole folder.
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if exclude.iter().any(|pattern| matches(pattern, &name)) {
                return None;
            }
            let path = entry.path();
            Some(Entry {
                name,
                // Through the link, so a junction to a folder unfolds like one.
                dir: path.is_dir(),
                path: path.to_string_lossy().into_owned(),
            })
        })
        .collect();

    sort(&mut entries);
    Ok(entries)
}

/// When a directory's own contents last changed, in milliseconds since the
/// Unix epoch; `None` if it is gone.
///
/// A directory's modification time moves when something is added to, removed
/// from or renamed in it — exactly the changes that make a listing stale — so
/// the tree can notice them for the cost of a `stat`.
pub fn dir_stamp(dir: &Path) -> Option<u64> {
    let meta = std::fs::metadata(dir).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(modified.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dot_star_pattern_is_every_dotfile_and_nothing_else() {
        assert!(matches(".*", ".git"));
        assert!(matches(".*", ".gitignore"));
        assert!(matches(".*", "."));
        assert!(!matches(".*", "src"));
        assert!(
            !matches(".*", "notes.txt"),
            "a dot later on is not a dotfile"
        );
    }

    #[test]
    fn stars_and_question_marks_are_wildcards() {
        assert!(matches("*.log", "build.log"));
        assert!(!matches("*.log", "build.logs"));
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "ac"));
        assert!(matches("*", "anything"));
        assert!(matches("a*b*c", "a-then-b-then-c"));
    }

    #[test]
    fn a_plain_pattern_is_an_exact_name() {
        assert!(matches("node_modules", "node_modules"));
        assert!(!matches("node_modules", "node_modules_old"));
        assert!(!matches("target", "targets"));
    }

    #[test]
    fn matching_ignores_case() {
        assert!(matches("Target", "TARGET"));
        assert!(matches("*.LOG", "build.log"));
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn folders_come_first_and_names_sort_without_case() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["b.txt", "A.txt", "c.txt"] {
            std::fs::write(dir.path().join(file), "").unwrap();
        }
        for folder in ["zeta", "Alpha"] {
            std::fs::create_dir(dir.path().join(folder)).unwrap();
        }

        let entries = list(dir.path(), &[]).unwrap();
        assert_eq!(
            names(&entries),
            ["Alpha", "zeta", "A.txt", "b.txt", "c.txt"]
        );
        assert!(entries[0].dir && !entries[2].dir);
    }

    #[test]
    fn excluded_names_are_left_out_whether_file_or_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join(".env"), "").unwrap();
        std::fs::write(dir.path().join("main.rs"), "").unwrap();

        let all = list(dir.path(), &[]).unwrap();
        assert_eq!(names(&all), [".git", "src", ".env", "main.rs"]);

        let visible = list(dir.path(), &[".*".to_owned()]).unwrap();
        assert_eq!(names(&visible), ["src", "main.rs"]);
    }

    #[test]
    fn a_folder_that_is_not_there_is_an_error_naming_it() {
        let err = list(Path::new("Z:/no/such/folder"), &[]).unwrap_err();
        assert!(err.to_string().contains("folder"), "got {err}");
    }

    #[test]
    fn a_directory_has_a_stamp_and_a_file_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "").unwrap();

        assert!(dir_stamp(dir.path()).is_some());
        assert_eq!(dir_stamp(&file), None);
        assert_eq!(dir_stamp(&dir.path().join("missing")), None);
    }
}
