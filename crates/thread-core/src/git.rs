//! What a folder's repository is up to, for the bottom bar: the branch it is
//! on, and how much in it is changed, staged, or not yet pushed.
//!
//! The counts are `git status`'s own, asked for in the form it prints for
//! programs. Where there is no `git` to ask, the branch alone is read
//! straight out of the repository's `HEAD`: one small file, which costs no
//! process and reads the same over SFTP as from a local disk.
//!
//! Only ever read. Nothing here stages, commits or fetches.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

/// No console window for a `git` that is a console program.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What `git` is asked: the state of the work, for a program to read.
/// Without the locks it would otherwise take to tidy the index as it goes,
/// so asking in the background never gets in the way of a `git` being typed.
pub const STATUS: &str = "git --no-optional-locks status --porcelain=v2 --branch";

/// A repository's state, as the bottom bar shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    /// The branch checked out, or for a detached head the start of its hash.
    pub branch: String,
    /// False when only the branch is known: there was no `git` to count with.
    pub counted: bool,
    /// Files with changes that are staged.
    pub staged: u32,
    /// Files with changes that are not.
    pub unstaged: u32,
    /// Files `git` has never been told of.
    pub untracked: u32,
    /// Files left in conflict by a merge.
    pub conflicts: u32,
    /// Whether the branch follows one on a remote, to be ahead of or behind.
    pub upstream: bool,
    /// Commits here that have not been pushed.
    pub ahead: u32,
    /// Commits there that have not been pulled.
    pub behind: u32,
}

impl Status {
    /// The branch and nothing else.
    pub fn of_branch(branch: String) -> Self {
        Self {
            branch,
            ..Self::default()
        }
    }
}

/// What [`STATUS`] printed, read. `None` if it is not that: the folder is
/// in no repository, or what came back is something else's output.
pub fn parse_status(printed: &str) -> Option<Status> {
    let mut status = Status {
        counted: true,
        ..Status::default()
    };
    let mut oid = None;
    let mut head = None;

    for line in printed.lines() {
        if let Some(header) = line.strip_prefix("# branch.") {
            let (key, value) = header.split_once(' ')?;
            match key {
                "oid" => oid = Some(value),
                "head" => head = Some(value),
                "upstream" => status.upstream = true,
                "ab" => {
                    let (ahead, behind) = value.split_once(' ')?;
                    status.ahead = ahead.trim_start_matches('+').parse().ok()?;
                    status.behind = behind.trim_start_matches('-').parse().ok()?;
                }
                _ => {}
            }
            continue;
        }
        // Whatever a login shell says before `git` does is not an entry.
        if head.is_none() {
            continue;
        }
        let mut fields = line.split(' ');
        match fields.next() {
            Some("1" | "2") => {
                // Two letters: what is staged, then what is not; a dot for neither.
                let mut state = fields.next()?.chars();
                status.staged += u32::from(state.next()? != '.');
                status.unstaged += u32::from(state.next()? != '.');
            }
            Some("u") => status.conflicts += 1,
            Some("?") => status.untracked += 1,
            _ => {}
        }
    }

    status.branch = match head? {
        "(detached)" => oid?.get(..7)?.to_owned(),
        branch => branch.to_owned(),
    };
    Some(status)
}

/// The state of the repository `dir` is in, or `None` outside any. With no
/// `git` installed it is the branch alone.
pub fn status(dir: &Path) -> Option<Status> {
    let mut words = STATUS.split(' ');
    let asked = Command::new(words.next()?)
        .args(words)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match asked {
        Ok(output) => parse_status(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => branch(dir).map(Status::of_branch),
    }
}

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
    fn a_status_counts_what_is_staged_changed_new_and_unpushed() {
        let printed = "Welcome to the machine\n\
            # branch.oid 4ae8666b1c0d9f2e7a6b5c4d3e2f1a0b9c8d7e6f\n\
            # branch.head main\n\
            # branch.upstream origin/main\n\
            # branch.ab +2 -1\n\
            1 M. N... 100644 100644 100644 aaa bbb staged.rs\n\
            1 .M N... 100644 100644 100644 aaa bbb changed.rs\n\
            1 MM N... 100644 100644 100644 aaa bbb both.rs\n\
            2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\told.rs\n\
            u UU N... 100644 100644 100644 100644 aaa bbb ccc fight.rs\n\
            ? notes.txt\n";
        assert_eq!(
            parse_status(printed),
            Some(Status {
                branch: "main".into(),
                counted: true,
                staged: 3,
                unstaged: 2,
                untracked: 1,
                conflicts: 1,
                upstream: true,
                ahead: 2,
                behind: 1,
            })
        );
    }

    #[test]
    fn a_detached_head_is_its_hash_and_nothing_at_all_is_no_repository() {
        let detached = "# branch.oid 4ae8666b1c0d9f2e7a6b5c4d3e2f1a0b9c8d7e6f\n# branch.head (detached)\n";
        let status = parse_status(detached).unwrap();
        assert_eq!(status.branch, "4ae8666");
        assert!(!status.upstream);
        assert_eq!(parse_status(""), None);
        assert_eq!(parse_status("fatal: not a git repository\n"), None);
    }

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
