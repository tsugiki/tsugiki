use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn sha(text: &str) -> CommitSha {
    CommitSha::parse(text.trim()).unwrap()
}

#[expect(
    clippy::disallowed_types,
    reason = "tests build their repositories with the real git (ADR 0009)"
)]
fn git_in(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=Contributor",
            "-c",
            "user.email=contributor@example.com",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn numbered(lines: std::ops::RangeInclusive<u32>) -> String {
    lines.map(|n| format!("line {n}\n")).collect()
}

/// A mirror with a branch `main`, and a PR branch `pr` started from main's first commit. main has moved on
/// since, in a file the PR doesn't touch.
pub(crate) struct Mirrored {
    root: PathBuf,
    pub(crate) mirror: PathBuf,
    pub(crate) head: CommitSha,
}

impl Mirrored {
    pub(crate) fn new(name: &str) -> Self {
        let root = crate::testing::temp_path(&format!("diff-{name}"));
        let work = root.join("work");
        for folder in ["Lib", "Modules", "Doc", "Tools"] {
            fs::create_dir_all(work.join(folder)).unwrap();
        }
        let _ = git_in(&work, &["init", "-q", "-b", "main"]);
        fs::write(work.join("Lib/ast.py"), numbered(1..=10)).unwrap();
        fs::write(work.join("Modules/old.c"), "int x;\n").unwrap();
        fs::write(work.join("Doc/notes.txt"), numbered(1..=20)).unwrap();
        fs::write(work.join("logo.png"), b"\x89PNG\0\x01").unwrap();
        fs::write(work.join("Tools/run.sh"), "echo hi\n").unwrap();
        let _ = git_in(&work, &["add", "."]);
        let _ = git_in(&work, &["commit", "-qm", "One"]);

        let _ = git_in(&work, &["switch", "-qc", "pr"]);
        let ast = numbered(1..=10).replace("line 5\n", "line five\n") + "line 11";
        fs::write(work.join("Lib/ast.py"), ast).unwrap();
        let _ = git_in(&work, &["rm", "-q", "Modules/old.c"]);
        let _ = git_in(&work, &["mv", "Doc/notes.txt", "Doc/news.txt"]);
        let news = numbered(1..=20).replace("line 20\n", "line twenty\n");
        fs::write(work.join("Doc/news.txt"), news).unwrap();
        fs::write(work.join("logo.png"), b"\x89PNG\0\x02").unwrap();
        fs::write(work.join("Lib/spaced name ü.py"), "pass\n").unwrap();
        let _ = git_in(&work, &["add", "."]);
        let _ = git_in(&work, &["update-index", "--chmod=+x", "Tools/run.sh"]);
        let _ = git_in(&work, &["commit", "-qm", "The PR"]);

        let _ = git_in(&work, &["switch", "-q", "main"]);
        fs::write(work.join("README.rst"), "Moved on\n").unwrap();
        let _ = git_in(&work, &["add", "README.rst"]);
        let _ = git_in(&work, &["commit", "-qm", "Two"]);

        let mirror = root.join("mirror.git");
        let _ = git_in(
            &root,
            &[
                "clone",
                "-q",
                "--bare",
                &work.to_string_lossy(),
                &mirror.to_string_lossy(),
            ],
        );
        let head = sha(&git_in(&mirror, &["rev-parse", "refs/heads/pr"]));
        Self { root, mirror, head }
    }

    /// Its diff, read from where it was made.
    pub(crate) fn diff(&self) -> Diff {
        read(&self.mirror, "main", &self.head).unwrap()
    }

    pub(crate) fn finish(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}

/// The rows as a reviewer reads them: old and new line numbers, a marker, and the text.
fn drawn(diff: &Diff) -> String {
    let mut out = String::new();
    for row in diff.rows(0..diff.row_count()) {
        let line = match row {
            DiffRow::File { file } => {
                let file = &diff.files()[usize::try_from(*file).unwrap()];
                let from = file
                    .old_path
                    .as_ref()
                    .map(|old| format!("{old} → "))
                    .unwrap_or_default();
                format!(
                    "=== {from}{} {:?} at row {}",
                    file.path, file.change, file.row
                )
            }
            DiffRow::Hunk { text } => text.clone(),
            DiffRow::Context { old, new, text } => format!("{old:>3} {new:>3}   {text}"),
            DiffRow::Added { new, text } => format!("    {new:>3} + {text}"),
            DiffRow::Removed { old, text } => format!("{old:>3}     - {text}"),
            DiffRow::Note { text } => format!("        ({text})"),
        };
        writeln!(out, "{line}").unwrap();
    }
    out
}

#[test]
fn a_prs_diff_runs_from_the_merge_base_to_its_head_file_by_file() {
    let mirrored = Mirrored::new("whole");

    let diff = mirrored.diff();

    insta::assert_snapshot!(drawn(&diff));
    assert!(
        diff.files().iter().all(|file| file.path != "README.rst"),
        "main's later commit isn't the PR's"
    );
    mirrored.finish();
}

#[test]
fn a_mirror_holds_a_diff_only_with_both_the_head_and_the_target_branch() {
    let mirrored = Mirrored::new("held");
    let elsewhere = sha(&"1".repeat(40));

    assert_eq!(held(&mirrored.mirror, "main", &mirrored.head), Held::Both);
    assert_eq!(
        held(&mirrored.mirror, "3.14", &mirrored.head),
        Held::NoTarget
    );
    assert_eq!(held(&mirrored.mirror, "main", &elsewhere), Held::NoHead);
    mirrored.finish();
}

#[test]
fn a_diff_git_cant_read_says_so_in_plain_words() {
    let mirrored = Mirrored::new("unreadable");

    let problem = read(&mirrored.mirror, "3.14", &mirrored.head).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert_eq!(
        problem.message,
        "Tsugiki couldn't read this PR's diff from its mirror."
    );
    mirrored.finish();
}

#[test]
fn rows_asked_for_past_the_end_stop_there() {
    let mirrored = Mirrored::new("past-the-end");
    let diff = mirrored.diff();
    let rows = diff.row_count();

    assert_eq!(diff.rows(rows - 2..rows + 512).len(), 2);
    assert_eq!(diff.rows(rows + 512..rows + 1024), []);
    let (late, early) = (5, 2);
    assert_eq!(diff.rows(late..early), []);
    mirrored.finish();
}

#[test]
fn the_diff_on_show_answers_each_chunk_and_another_head_is_read_again() {
    let mirrored = Mirrored::new("on-show");
    let diffs = Diffs::default();
    let pr = PrAt {
        repository: RepositoryName::parse("python/cpython").unwrap(),
        number: 7,
        head: mirrored.head.clone(),
    };
    let moved = PrAt {
        head: sha(&"2".repeat(40)),
        ..pr.clone()
    };
    let other_pr = PrAt {
        number: 8,
        ..pr.clone()
    };

    let first = diffs.get_or_read(&pr, || Ok(mirrored.diff())).unwrap();
    let again = diffs.get_or_read(&pr, || panic!("read again")).unwrap();
    let other = diffs.get_or_read(&moved, || Ok(mirrored.diff())).unwrap();

    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(other.pr.head, moved.head);
    let failed = diffs.get_or_read(&other_pr, || Err(Error::new(ErrorKind::Internal, "no")));
    assert!(failed.is_err());
    mirrored.finish();
}

#[test]
fn a_patch_and_a_list_of_files_that_dont_agree_read_as_nothing() {
    let one = "1\t0\ta.py\0";
    let patch = "diff --git a/a.py b/a.py\n@@ -0,0 +1 @@\n+x\n";
    let hunk = |header: &str| format!("diff --git a/a.py b/a.py\n{header}\n+x\n");

    assert!(Diff::parse(one, patch).is_ok());
    assert!(Diff::parse("", patch).is_err(), "one file too many");
    assert!(Diff::parse("1\t0\0", patch).is_err(), "no path");
    assert!(Diff::parse("x\t0\ta.py\0", patch).is_err(), "no count");
    assert!(
        Diff::parse("1\t0\t\0a.py\0", patch).is_err(),
        "a rename with one path"
    );
    assert!(
        Diff::parse(one, &hunk("@@ -0,0 @@")).is_err(),
        "no new side"
    );
    assert!(
        Diff::parse(one, &hunk("@@ x +1 @@")).is_err(),
        "no old side"
    );
}

#[test]
fn an_empty_line_of_context_without_its_space_is_still_context() {
    let patch = "diff --git a/a.py b/a.py\n@@ -1,2 +1,2 @@\n\n-x\n+y\n";

    let diff = Diff::parse("1\t1\ta.py\0", patch).unwrap();

    assert_eq!(
        diff.rows(2..3),
        [DiffRow::Context {
            old: 1,
            new: 1,
            text: String::new()
        }]
    );
}
