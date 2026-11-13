//! Text search over a PR's code, on a tree about the size of CPython's (#685). On CPython itself a literal
//! search of the whole tree should end in about 0.2 s, as ripgrep did in ADR 0020's spike.

use std::fs;
use std::path::{Path, PathBuf};

use tsugiki_core::github::CommitSha;
use tsugiki_core::mirror::{LookupAt, LookupCheckout};
use tsugiki_core::search::{Case, End, Query, Syntax, Words, search_checkout};
use tsugiki_test_git::git;

fn main() {
    divan::main();
}

/// The files of the tree, as many as CPython's checkout held in the spike.
const FILES: u32 = 6_000;

/// The lines of each file, which make about 60 MB in all.
const LINES: u32 = 250;

/// The files the PR changes, read from git's objects rather than from disk.
const CHANGED: u32 = 50;

#[expect(
    clippy::expect_used,
    reason = "a benchmark that found nothing would measure a search the user doesn't wait for"
)]
#[divan::bench]
fn literal(bencher: divan::Bencher<'_, '_>) {
    let tree = Tree::new("literal");
    let query = Query {
        text: "PyErr_SetString".to_owned(),
        case: Case::Ignored,
        words: Words::Within,
        syntax: Syntax::Literal,
    };
    let (files, end) = search_checkout(&tree.checkout, &query).expect("a search");
    assert!(
        files.len() > 50 && end == End::Done,
        "{} files",
        files.len()
    );
    bencher.bench(|| search_checkout(&tree.checkout, &query));
    tree.remove();
}

#[expect(
    clippy::expect_used,
    reason = "a benchmark that found nothing would measure a search the user doesn't wait for"
)]
#[divan::bench]
fn regex(bencher: divan::Bencher<'_, '_>) {
    let tree = Tree::new("regex");
    let query = Query {
        text: r".*GetAttr\w*\(".to_owned(),
        case: Case::Matched,
        words: Words::Within,
        syntax: Syntax::Regex,
    };
    let (files, end) = search_checkout(&tree.checkout, &query).expect("a search");
    assert!(
        files.len() > 50 && end == End::Done,
        "{} files",
        files.len()
    );
    bencher.bench(|| search_checkout(&tree.checkout, &query));
    tree.remove();
}

/// A repository at a PR's base, with the PR's head in its objects, as a Lookup checkout holds it.
struct Tree {
    folder: PathBuf,
    checkout: LookupCheckout,
}

impl Tree {
    #[expect(
        clippy::expect_used,
        reason = "a benchmark whose tree can't be built has nothing to measure"
    )]
    fn new(name: &str) -> Self {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.temp")
            .join(format!("bench-search-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&folder));
        fs::create_dir_all(&folder).expect("a folder for the tree");
        let _ = git(&folder, &["init", "-q", "-b", "main"]);
        let _ = git(&folder, &["config", "core.autocrlf", "false"]);
        for file in 0..FILES {
            write(&folder, file, "base");
        }
        let base = commit(&folder, "base");
        for file in 0..CHANGED {
            write(&folder, file * (FILES / CHANGED), "head");
        }
        let head = commit(&folder, "head");
        let _ = git(&folder, &["checkout", "-q", "--detach", base.as_str()]);
        Self {
            checkout: LookupCheckout {
                folder: folder.clone(),
                at: LookupAt { base, head },
            },
            folder,
        }
    }

    fn remove(self) {
        // A folder left behind sits in the ignored `.temp`, and the next run gets a new one.
        drop(fs::remove_dir_all(&self.folder));
    }
}

/// Writes file number `file`, C or Python by turns, in folders of 100. One in 20 of its lines calls
/// `PyErr_SetString`, in one file in 50, and three call `PyObject_GetAttrString`, in one file in 10. Both
/// searches then find fewer than `MOST_MATCHES` and read the whole tree.
#[expect(
    clippy::expect_used,
    reason = "a benchmark whose tree can't be built has nothing to measure"
)]
fn write(folder: &Path, file: u32, version: &str) {
    let (directory, extension) = if file.is_multiple_of(2) {
        ("Lib", "py")
    } else {
        ("Objects", "c")
    };
    let path = folder.join(format!(
        "{directory}/part{}/file{file}.{extension}",
        file / 100
    ));
    fs::create_dir_all(path.parent().unwrap_or(folder)).expect("a folder");
    let calls = file.is_multiple_of(50);
    let gets = file.is_multiple_of(10);
    let text: String = (0..LINES)
        .map(|line| {
            if calls && line.is_multiple_of(20) {
                format!("    PyErr_SetString(PyExc_ValueError, \"{version} {line}\");\n")
            } else if gets && line.is_multiple_of(97) {
                format!("    result = PyObject_GetAttrString(obj, \"name_{line}\");\n")
            } else {
                format!("    value_{line} = compute(argument_{line}, {version:?}) + {file}\n")
            }
        })
        .collect();
    fs::write(path, text).expect("a file of the tree");
}

#[expect(clippy::expect_used, reason = "git names a commit by its SHA")]
fn commit(folder: &Path, message: &str) -> CommitSha {
    let _ = git(folder, &["add", "-A"]);
    let _ = git(folder, &["commit", "-q", "-m", message]);
    let commit = git(folder, &["rev-parse", "HEAD"]);
    CommitSha::parse(commit.trim()).expect("a commit's SHA")
}
