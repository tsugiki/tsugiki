//! Commits that one `git fast-import` writes, where adds and commits in a working copy would start a few gits
//! each: about 30 ms for each git started on Windows. The repository's working tree and index stay as they
//! were, as a Mirror has none.

use std::path::Path;

use tsugiki_test_git::git_reading;

use super::sha;
use crate::facts::CommitSha;

/// A commit for [`import`] to write.
pub(crate) struct Commit<'a> {
    /// The branch it goes on. fast-import refuses to move a branch to a commit that doesn't descend from it,
    /// so a rebase goes on a branch of its own.
    pub(crate) branch: &'a str,
    pub(crate) message: &'a str,
    /// The commits it follows, the first parent first. Its tree is the first parent's with `changes` made.
    /// With none, it follows the commit the same import last put on `branch`, or starts a history: on a
    /// branch an earlier import wrote, fast-import refuses that.
    pub(crate) parents: &'a [&'a CommitSha],
    pub(crate) changes: &'a [Change<'a>],
}

/// A change a [`Commit`] makes to its first parent's tree.
pub(crate) enum Change<'a> {
    /// Writes the bytes at the path, as an entry of the mode.
    Write(Mode, &'a str, &'a [u8]),
    /// Puts a submodule at the path, at the commit.
    Submodule(&'a str, &'a CommitSha),
    /// Moves the file at the first path to the second.
    Rename(&'a str, &'a str),
    Delete(&'a str),
}

/// What a [`Change::Write`] puts in the tree.
pub(crate) enum Mode {
    File,
    Executable,
    /// A symlink, to the path its bytes name.
    Symlink,
}

impl Mode {
    fn octal(&self) -> &'static str {
        match self {
            Self::File => "100644",
            Self::Executable => "100755",
            Self::Symlink => "120000",
        }
    }
}

/// Writes `commits` into the repository at `repository`, in order, and gives each.
pub(crate) fn import(repository: &Path, commits: &[Commit<'_>]) -> Vec<CommitSha> {
    let mut stream = Vec::new();
    for (mark, commit) in (1..).zip(commits) {
        commit_into(&mut stream, mark, commit);
    }
    // fast-import prints the commit of each mark it's asked for, a line each.
    for mark in 1..=commits.len() {
        stream.extend_from_slice(format!("get-mark :{mark}\n").as_bytes());
    }
    let args = ["fast-import", "--quiet", "--date-format=now"];
    let printed = git_reading(repository, &args, &stream);
    printed.lines().map(sha).collect()
}

/// Adds `commit` to `stream`, marked `mark` for `get-mark` to name.
fn commit_into(stream: &mut Vec<u8>, mark: usize, commit: &Commit<'_>) {
    let Commit {
        branch,
        message,
        parents,
        changes,
    } = commit;
    let author = tsugiki_test_git::committer();
    let length = message.len();
    let header = format!("commit refs/heads/{branch}\nmark :{mark}\ncommitter {author} now\n");
    stream.extend_from_slice(header.as_bytes());
    stream.extend_from_slice(format!("data {length}\n{message}\n").as_bytes());
    for (at, parent) in parents.iter().enumerate() {
        let word = if at == 0 { "from" } else { "merge" };
        stream.extend_from_slice(format!("{word} {}\n", parent.as_str()).as_bytes());
    }
    for change in *changes {
        match change {
            Change::Write(mode, path, bytes) => {
                let (mode, length) = (mode.octal(), bytes.len());
                let line = format!("M {mode} inline {path}\ndata {length}\n");
                stream.extend_from_slice(line.as_bytes());
                stream.extend_from_slice(bytes);
                stream.push(b'\n');
            }
            Change::Submodule(path, at) => {
                let line = format!("M 160000 {} {path}\n", at.as_str());
                stream.extend_from_slice(line.as_bytes());
            }
            Change::Rename(from, to) => {
                stream.extend_from_slice(format!("R {from} {to}\n").as_bytes())
            }
            Change::Delete(path) => stream.extend_from_slice(format!("D {path}\n").as_bytes()),
        }
    }
    stream.push(b'\n');
}
