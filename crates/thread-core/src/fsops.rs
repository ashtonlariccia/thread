//! Creating, renaming and deleting, for the file tree.
//!
//! Every operation here refuses rather than overwrites. The tree acts on a
//! name typed into a small box; nothing typed there should be able to destroy
//! a file that already exists.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

fn refuse(message: impl Into<String>) -> Error {
    Error::Other(anyhow::anyhow!(message.into()))
}

/// Check a name typed for a new or renamed entry.
///
/// One path component, and one Windows will accept: the tree creates things
/// *in* a folder, so a name that wanders elsewhere (`..\x`, `a/b`) is a
/// mistake rather than a feature.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(refuse("A name is needed."));
    }
    if name == "." || name == ".." {
        return Err(refuse(format!(
            "\"{name}\" is not a name that can be used."
        )));
    }
    if let Some(bad) = name.chars().find(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err(refuse(format!("A name cannot contain {bad:?}.")));
    }
    // Windows silently strips these, so the entry created would not be the
    // one asked for.
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(refuse("A name cannot end with a space or a full stop."));
    }
    Ok(())
}

fn child(dir: &Path, name: &str) -> Result<PathBuf> {
    validate_name(name)?;
    Ok(dir.join(name))
}

fn exists_error(path: &Path) -> Error {
    refuse(format!(
        "\"{}\" already exists.",
        path.file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
    ))
}

/// Create an empty file called `name` in `dir`.
pub fn create_file(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = child(dir, name)?;
    std::fs::OpenOptions::new()
        .write(true)
        // Atomic "only if it is not there": no window in which a file that
        // appears between a check and the create gets truncated.
        .create_new(true)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => exists_error(&path),
            _ => refuse(format!("could not create {}: {e}", path.display())),
        })?;
    Ok(path)
}

/// Create a folder called `name` in `dir`.
pub fn create_dir(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = child(dir, name)?;
    std::fs::create_dir(&path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => exists_error(&path),
        _ => refuse(format!("could not create {}: {e}", path.display())),
    })?;
    Ok(path)
}

/// Give a file or folder a new name, in the folder it is already in.
pub fn rename(path: &Path, name: &str) -> Result<PathBuf> {
    let dir = path
        .parent()
        .ok_or_else(|| refuse(format!("{} cannot be renamed.", path.display())))?;
    let target = child(dir, name)?;

    // `rename` replaces an existing file without a word. The one "existing"
    // target that is fine is the entry itself, which is what changing only
    // the case of a name looks like on a case-insensitive disk.
    if target.exists() && !same_entry(path, &target) {
        return Err(exists_error(&target));
    }
    std::fs::rename(path, &target)
        .map_err(|e| refuse(format!("could not rename {}: {e}", path.display())))?;
    Ok(target)
}

fn same_entry(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Move a file or folder to the Recycle Bin.
///
/// The bin rather than gone for good: a tree makes deleting a folder one
/// keypress and a confirmation, and that should be something you can undo.
pub fn delete(path: &Path) -> Result<()> {
    trash::delete(path).map_err(|e| refuse(format!("could not delete {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_accepted() {
        for name in [
            "main.rs",
            ".gitignore",
            "My File (2).txt",
            "über.md",
            "a.b.c",
        ] {
            assert!(validate_name(name).is_ok(), "{name}");
        }
    }

    #[test]
    fn names_that_would_land_somewhere_else_are_refused() {
        for name in ["", ".", "..", "a/b", "a\\b", "..\\escape", "C:x"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn names_windows_would_quietly_change_are_refused() {
        for name in ["trailing ", "trailing.", "what?", "a*b", "tab\tname"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn a_new_file_is_created_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_file(dir.path(), "a.txt").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"");
    }

    /// The whole point of refusing: typing an existing name must not empty it.
    #[test]
    fn creating_over_an_existing_file_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "precious").unwrap();

        let err = create_file(dir.path(), "a.txt").unwrap_err().to_string();
        assert!(err.contains("already exists"), "got {err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "precious");
    }

    #[test]
    fn a_new_folder_is_created_and_not_twice() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_dir(dir.path(), "src").unwrap();
        assert!(path.is_dir());
        assert!(create_dir(dir.path(), "src").is_err());
    }

    #[test]
    fn renaming_keeps_the_contents_and_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.txt");
        std::fs::write(&old, "text").unwrap();

        let new = rename(&old, "new.txt").unwrap();
        assert_eq!(new, dir.path().join("new.txt"));
        assert!(!old.exists());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "text");
    }

    #[test]
    fn renaming_onto_another_file_is_refused_and_harms_neither() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();

        assert!(rename(&a, "b.txt").is_err());
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
    }

    #[test]
    fn a_name_can_be_changed_in_case_alone() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("readme.md");
        std::fs::write(&old, "x").unwrap();

        let new = rename(&old, "README.md").unwrap();
        let listed: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(listed, ["README.md"]);
        assert_eq!(std::fs::read_to_string(new).unwrap(), "x");
    }

    #[test]
    fn a_folder_can_be_renamed_with_what_is_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("inner.txt"), "in").unwrap();

        let new = rename(&old, "new").unwrap();
        assert_eq!(
            std::fs::read_to_string(new.join("inner.txt")).unwrap(),
            "in"
        );
    }
}
