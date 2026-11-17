//! The Repository with a PR that the searches and the language servers work in, which only the core's own
//! tests build: it needs `tsugiki-test-git`.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use tsugiki_test_git::{git, git_reading};

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
        // What `git config core.autocrlf false` writes, without a git to write it. The checkout reads it, so
        // it holds the bytes the commits do on a machine whose git turns LF into CRLF.
        let mut config = fs::OpenOptions::new()
            .append(true)
            .open(mirror.join(".git").join("config"))
            .unwrap();
        config.write_all(b"[core]\n\tautocrlf = false\n").unwrap();
        let (base_commit, head_commit) = import(&root, &mirror, base, head);
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

/// Commits `base` to `main` in the repository at `mirror`, then `head` on top of it, with one `git
/// fast-import`: about 30 ms for each git started on Windows, where adds and commits would start six. Gives
/// the two commits. The repository's working tree and index stay empty, as a Mirror has none.
fn import(
    root: &Path,
    mirror: &Path,
    base: &[(&str, &[u8])],
    head: &[(&str, Option<&[u8]>)],
) -> (CommitSha, CommitSha) {
    let mut stream = Vec::new();
    let base_files = base.iter().map(|(path, bytes)| (*path, Some(*bytes)));
    commit_into(&mut stream, 1, "base", base_files);
    commit_into(&mut stream, 2, "head", head.iter().copied());
    let marks = root.join("marks");
    let export = format!("--export-marks={}", marks.display());
    let args = ["fast-import", "--quiet", "--date-format=now", &export];
    let _ = git_reading(mirror, &args, &stream);
    let marks = fs::read_to_string(marks).unwrap();
    let commit = |mark: &str| {
        let found = marks.lines().find_map(|line| line.strip_prefix(mark));
        sha(found.unwrap())
    };
    (commit(":1 "), commit(":2 "))
}

/// Adds to `stream` a commit on `main` that writes each file with bytes and deletes each without, marked
/// `mark` for `--export-marks` to name.
fn commit_into<'a>(
    stream: &mut Vec<u8>,
    mark: u32,
    message: &str,
    files: impl Iterator<Item = (&'a str, Option<&'a [u8]>)>,
) {
    let author = "Contributor <contributor@example.com>";
    let length = message.len();
    let header = format!("commit refs/heads/main\nmark :{mark}\ncommitter {author} now\n");
    stream.extend_from_slice(header.as_bytes());
    stream.extend_from_slice(format!("data {length}\n{message}\n").as_bytes());
    for (path, bytes) in files {
        match bytes {
            Some(bytes) => {
                let length = bytes.len();
                let line = format!("M 100644 inline {path}\ndata {length}\n");
                stream.extend_from_slice(line.as_bytes());
                stream.extend_from_slice(bytes);
                stream.push(b'\n');
            }
            None => stream.extend_from_slice(format!("D {path}\n").as_bytes()),
        }
    }
    stream.push(b'\n');
}
