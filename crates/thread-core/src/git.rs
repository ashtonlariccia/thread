//! Which branch a folder is on, for the bottom bar.
//!
//! Read straight out of the repository's `HEAD`, without running `git`: it is
//! one small file, and asking for it this way costs no process and works the
//! same over SFTP as on a local disk.

use std::path::Path;

/// How far up from a folder a repository is looked for.
pub const MAX_DEPTH: usize = 32;

/// What `HEAD` says is checked out: a branch's name, or for a detached head
/// the start of the commit's hash.
pub fn head_label(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return (!branch.is_empty()).then(|| branch.to_owned());
    }
    // A hash, and nothing that merely is not a ref.
    (head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit())).then(|| head[..7].to_owned())
}

/// Where a `.git` *file* says the repository really is. A worktree and a
/// submodule have one in place of the folder.
pub fn gitdir_of(pointer: &str) -> Option<&str> {
    pointer.trim().strip_prefix("gitdir:").map(str::trim)
}

/// The branch `dir` is on: its own repository's, or that of the nearest
/// folder above it that has one. `None` outside any repository.
pub fn branch(dir: &Path) -> Option<String> {
    for folder in dir.ancestors().take(MAX_DEPTH) {
        let dot_git = folder.join(".git");
        if dot_git.is_dir() {
            return head_label(&std::fs::read_to_string(dot_git.join("HEAD")).ok()?);
        }
        if dot_git.is_file() {
            let pointer = std::fs::read_to_string(&dot_git).ok()?;
            // Relative to the folder the pointer is in, if it is relative.
            let gitdir = folder.join(gitdir_of(&pointer)?);
            return head_label(&std::fs::read_to_string(gitdir.join("HEAD")).ok()?);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_is_named_and_a_detached_head_is_its_hash() {
        assert_eq!(
            head_label("ref: refs/heads/main\n").as_deref(),
            Some("main")
        );
        assert_eq!(
            head_label("ref: refs/heads/feature/tabs\n").as_deref(),
            Some("feature/tabs")
        );
        assert_eq!(
            head_label("4ae8666b1c0d9f2e7a6b5c4d3e2f1a0b9c8d7e6f\n").as_deref(),
            Some("4ae8666")
        );
        assert_eq!(head_label("garbage"), None);
        assert_eq!(head_label(""), None);
    }

    #[test]
    fn a_folder_inside_a_repository_is_on_its_branch() {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join(".git")).unwrap();
        std::fs::write(repo.path().join(".git/HEAD"), "ref: refs/heads/dev\n").unwrap();
        let deep = repo.path().join("src/lib");
        std::fs::create_dir_all(&deep).unwrap();

        assert_eq!(branch(repo.path()).as_deref(), Some("dev"));
        assert_eq!(branch(&deep).as_deref(), Some("dev"));
    }

    /// A worktree's `.git` is a file that points at where its `HEAD` is.
    #[test]
    fn a_worktree_is_followed_to_its_own_head() {
        let root = tempfile::tempdir().unwrap();
        let gitdir = root.path().join("repo/.git/worktrees/wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/wt-branch\n").unwrap();

        let worktree = root.path().join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();

        assert_eq!(branch(&worktree).as_deref(), Some("wt-branch"));
    }
}
