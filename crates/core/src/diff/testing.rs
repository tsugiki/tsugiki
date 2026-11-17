//! A mirror with a PR, built with the real git (ADR 0009), whose diff the tests read.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tsugiki_test_git::{Snapshot, git};

use super::{Diff, MirrorDiff, Start, read};
use crate::facts::CommitSha;
use crate::testing::{TempPath, sha};

fn numbered(lines: std::ops::RangeInclusive<u32>) -> String {
    lines.map(|n| format!("line {n}\n")).collect()
}

/// Python that sets `x1` to 1, and so on.
fn assigned(lines: std::ops::RangeInclusive<u32>) -> String {
    lines.map(|n| format!("x{n} = {n}\n")).collect()
}

/// A mirror with a branch `main`, and a PR branch `pr` started from main's first commit. main has moved on
/// since, in a file the PR doesn't touch.
pub(crate) struct Mirrored {
    pub(crate) mirror: PathBuf,
    pub(crate) head: CommitSha,
    pub(super) root: TempPath,
}

/// A NEWS entry main has before the PR starts, which a PR can delete.
pub(super) const OLD_NEWS: &str = "Misc/NEWS.d/next/Library/2026-01-01-00-00-00.gh-issue-2.Old.rst";

/// The mirror [`Mirrored::new`] copies, and its PR's head. About 15 gits build it, once in a test run.
static BUILT: OnceLock<(Snapshot, CommitSha)> = OnceLock::new();

impl Mirrored {
    /// A copy of the mirror, which the test can change: the copy shares nothing with another test's.
    pub(crate) fn new(name: &str) -> Self {
        let (snapshot, head) = BUILT.get_or_init(|| {
            let built = Self::with("built", |_| {});
            // A copy would point `origin` at the working copy, which goes once the mirror is read.
            let _ = git(&built.mirror, &["remote", "remove", "origin"]);
            (Snapshot::of(&built.mirror), built.head)
        });
        let root = crate::testing::temp_path(&format!("diff-{name}"));
        let mirror = root.join("mirror.git");
        snapshot.write(&mirror);
        Self {
            mirror,
            head: head.clone(),
            root,
        }
    }

    /// A mirror built for this test alone, with what `more` adds to the PR's commit in the folder it gets.
    pub(super) fn with(name: &str, more: impl FnOnce(&Path)) -> Self {
        let root = crate::testing::temp_path(&format!("diff-{name}"));
        let work = root.join("work");
        for folder in ["Lib", "Modules", "Doc", "Tools"] {
            fs::create_dir_all(work.join(folder)).unwrap();
        }
        let _ = git(&work, &["init", "-q", "-b", "main"]);
        // The PR sets run.sh's mode in the index only, which macOS's git would then see as a change on disk.
        let _ = git(&work, &["config", "core.fileMode", "false"]);
        fs::write(work.join("Lib/ast.py"), assigned(1..=10)).unwrap();
        fs::write(work.join("Modules/old.c"), "int x;\n").unwrap();
        fs::write(work.join("Doc/notes.txt"), numbered(1..=20)).unwrap();
        fs::write(work.join("logo.png"), b"\x89PNG\0\x01").unwrap();
        fs::write(work.join("Tools/run.sh"), "echo hi\n").unwrap();
        fs::create_dir_all(work.join("Misc/NEWS.d/next/Library")).unwrap();
        fs::write(work.join(OLD_NEWS), "Old news.\n").unwrap();
        let _ = git(&work, &["add", "."]);
        let _ = git(&work, &["commit", "-qm", "One"]);

        let _ = git(&work, &["switch", "-qc", "pr"]);
        let ast = assigned(1..=10).replace("x5 = 5\n", "x5 = 'five'\n") + "x11 = 11";
        fs::write(work.join("Lib/ast.py"), ast).unwrap();
        let _ = git(&work, &["rm", "-q", "Modules/old.c"]);
        let _ = git(&work, &["mv", "Doc/notes.txt", "Doc/news.txt"]);
        let news = numbered(1..=20).replace("line 20\n", "line twenty\n");
        fs::write(work.join("Doc/news.txt"), news).unwrap();
        fs::write(work.join("logo.png"), b"\x89PNG\0\x02").unwrap();
        fs::write(work.join("Lib/spaced name ü.py"), "pass\n").unwrap();
        more(&work);
        let _ = git(&work, &["add", "."]);
        let _ = git(&work, &["update-index", "--chmod=+x", "Tools/run.sh"]);
        let _ = git(&work, &["commit", "-qm", "The PR"]);

        let _ = git(&work, &["switch", "-q", "main"]);
        fs::write(work.join("README.rst"), "Moved on\n").unwrap();
        // Only main's newest commit marks files generated, so a diff reads them from there.
        let attributes = "[attr]generated linguist-generated=true\nLib/ast.py generated\n\
                          *.png linguist-generated\nDoc/** -linguist-generated\n";
        fs::write(work.join(".gitattributes"), attributes).unwrap();
        let _ = git(&work, &["add", "README.rst", ".gitattributes"]);
        let _ = git(&work, &["commit", "-qm", "Two"]);

        let mirror = root.join("mirror.git");
        let _ = git(
            &root,
            &[
                "clone",
                "-q",
                "--bare",
                &work.to_string_lossy(),
                &mirror.to_string_lossy(),
            ],
        );
        let head = sha(&git(&mirror, &["rev-parse", "refs/heads/pr"]));
        Self { mirror, head, root }
    }

    /// Its diff, read from where it was made.
    pub(crate) fn diff(&self) -> Diff {
        self.read().diff
    }

    /// Its diff and the merge base it starts from.
    pub(crate) fn read(&self) -> MirrorDiff {
        read(&self.mirror, "main", &Start::TargetBranch, &self.head).unwrap()
    }
}
