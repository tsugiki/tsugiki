//! The Repository with a PR that the searches and the language servers work in, which only the core's own
//! tests build: it needs `tsugiki-test-git`.

use std::fs;
use std::path::{Path, PathBuf};

use tsugiki_test_git::git;

use super::{TempPath, sha, temp_path};
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
        let _ = git(&mirror, &["config", "core.autocrlf", "false"]);
        for (path, bytes) in base {
            write(&mirror, path, bytes);
        }
        let base_commit = commit(&mirror, "base");
        let mut added = Vec::new();
        for (path, bytes) in head {
            match bytes {
                Some(bytes) => {
                    if !mirror.join(path).exists() {
                        added.push(*path);
                    }
                    write(&mirror, path, bytes);
                }
                None => fs::remove_file(mirror.join(path)).unwrap(),
            }
        }
        let head_commit = commit(&mirror, "head");
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

/// Writes the file for the next commit, which stages it with the rest: a `git add` here would start one git
/// for each file.
fn write(folder: &Path, path: &str, bytes: &[u8]) {
    let file = folder.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, bytes).unwrap();
}

fn commit(folder: &Path, message: &str) -> CommitSha {
    sha(&tsugiki_test_git::commit(folder, message))
}
