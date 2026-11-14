//! A Repository with a PR, a Lookup checkout at its base as the Mirror worker leaves one, built with the
//! real git (ADR 0009), and the ty the tests start.

use std::path::{Path, PathBuf};
use std::{env, fs};

use tsugiki_test_git::git;

use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::testing::temp_path;

/// The ty that `TSUGIKI_TY` names, which `just cargo-test` and `just rust-coverage` set to the one
/// `scripts/fetch-ty.ts` fetches.
pub(crate) fn ty() -> PathBuf {
    let ty = PathBuf::from(env::var_os("TSUGIKI_TY").unwrap_or_default());
    assert!(ty.is_file(), "TSUGIKI_TY={ty:?}: see just fetch-ty");
    ty
}

/// A repository whose `main` is the PR's base, and whose PR's head changes, adds and deletes files.
pub(crate) struct Fixture {
    /// The repository the PR is in, which stands in for its Mirror.
    pub(crate) mirror: PathBuf,
    pub(crate) checkout: LookupCheckout,
}

impl Fixture {
    /// The base holds `base`, and the PR's head writes each of `head`'s files, or deletes it where its text
    /// is None. The checkout holds the base and the head's added files, as a move leaves it.
    pub(crate) fn new(name: &str, base: &[(&str, &str)], head: &[(&str, Option<&str>)]) -> Self {
        // The folder keeps the `..` in `temp_path`'s path, which the core reads past.
        let root = temp_path(name);
        let mirror = root.join("mirror");
        fs::create_dir_all(&mirror).unwrap();
        let _ = git(&mirror, &["init", "-q", "-b", "main"]);
        let _ = git(&mirror, &["config", "core.autocrlf", "false"]);
        for (path, text) in base {
            write(&mirror, path, text);
        }
        let base_commit = commit(&mirror, "base");
        let mut added = Vec::new();
        for (path, text) in head {
            match text {
                Some(text) => {
                    if !mirror.join(path).exists() {
                        added.push(*path);
                    }
                    write(&mirror, path, text);
                }
                None => {
                    let _ = git(&mirror, &["rm", "-q", path]);
                }
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
        }
    }
}

fn write(folder: &Path, path: &str, text: &str) {
    let file = folder.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, text).unwrap();
    let _ = git(folder, &["add", path]);
}

fn commit(folder: &Path, message: &str) -> CommitSha {
    let _ = git(folder, &["commit", "-q", "--allow-empty", "-m", message]);
    let commit = git(folder, &["rev-parse", "HEAD"]);
    CommitSha::parse(commit.trim()).unwrap()
}
