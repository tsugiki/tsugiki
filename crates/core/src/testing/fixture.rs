//! The Repository with a PR that the searches and the language servers work in, which only the core's own
//! tests build: it needs `tsugiki-test-git`.

use std::fs;
use std::path::{Path, PathBuf};

use tsugiki_test_git::git;

use super::import::{self, Change, Commit, Mode};
use super::{TempPath, add_config, temp_path};
use crate::facts::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};

/// A repository whose `main` is a PR's base and whose PR's head changes, adds and deletes files, and a
/// Lookup checkout at its base as the Mirror worker leaves one, built with the real git (ADR 0009).
pub(crate) struct Fixture {
    /// The repository the PR is in, which stands in for its Mirror.
    pub(crate) mirror: PathBuf,
    pub(crate) checkout: LookupCheckout,
    /// The folder around both, which a test drops after the searches and servers that work in it.
    _root: TempPath,
}

impl Fixture {
    /// [`Fixture::of_bytes`] for files of text.
    pub(crate) fn new(name: &str, base: &[(&str, &str)], head: &[(&str, Option<&str>)]) -> Self {
        let base: Vec<(&str, &[u8])> = (base.iter())
            .map(|(path, text)| (*path, text.as_bytes()))
            .collect();
        let head: Vec<(&str, Option<&[u8]>)> = (head.iter())
            .map(|(path, text)| (*path, text.map(str::as_bytes)))
            .collect();
        Self::of_bytes(name, &base, &head)
    }

    /// The base holds `base`, and the PR's head writes each of `head`'s files, or deletes it where its bytes
    /// are None. The checkout holds the base and the head's added files, as a move leaves it.
    pub(crate) fn of_bytes(
        name: &str,
        base: &[(&str, &[u8])],
        head: &[(&str, Option<&[u8]>)],
    ) -> Self {
        // The folder keeps the `..` in `temp_path`'s path, which the core reads past.
        let root = temp_path(name);
        let mirror = root.join("mirror");
        fs::create_dir_all(&mirror).unwrap();
        let _ = git(&mirror, &["init", "-q", "-b", "main"]);
        // What `git config core.autocrlf false` writes, without a git to write it. The checkout reads it, so
        // it holds the bytes the commits do on a machine whose git turns LF into CRLF.
        add_config(&mirror.join(".git"), "[core]\n\tautocrlf = false\n");
        let (base_commit, head_commit) = import(&mirror, base, head);
        let added: Vec<&str> = (head.iter())
            .filter(|(path, bytes)| bytes.is_some() && !base.iter().any(|(old, _)| old == path))
            .map(|(path, _)| *path)
            .collect();
        let folder = root.join("lookup");
        let folder_arg = folder.to_string_lossy().into_owned();
        let at = [
            "worktree",
            "add",
            "-q",
            "--detach",
            &folder_arg,
            base_commit.as_str(),
        ];
        let _ = git(&mirror, &at);
        if !added.is_empty() {
            let mut args = vec!["checkout", head_commit.as_str(), "--"];
            args.extend(added);
            let _ = git(&folder, &args);
        }
        Self {
            mirror,
            checkout: LookupCheckout {
                folder,
                at: LookupAt {
                    base: base_commit,
                    head: head_commit,
                },
            },
            _root: root,
        }
    }
}

/// Commits `base` to `main` in the repository at `mirror`, then `head` on top of it, and gives the two
/// commits. `head` writes each file with bytes and deletes each without.
fn import(
    mirror: &Path,
    base: &[(&str, &[u8])],
    head: &[(&str, Option<&[u8]>)],
) -> (CommitSha, CommitSha) {
    let written: Vec<Change<'_>> = (base.iter())
        .map(|(path, bytes)| Change::Write(Mode::File, path, bytes))
        .collect();
    let changed: Vec<Change<'_>> = (head.iter())
        .map(|(path, bytes)| match bytes {
            Some(bytes) => Change::Write(Mode::File, path, bytes),
            None => Change::Delete(path),
        })
        .collect();
    let on_main = |message, changes| Commit {
        branch: "main",
        message,
        parents: &[],
        changes,
    };
    let commits = import::import(
        mirror,
        &[on_main("base", &written), on_main("head", &changed)],
    );
    let [base, head] = <[CommitSha; 2]>::try_from(commits).unwrap();
    (base, head)
}
