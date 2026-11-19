use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use tsugiki_test_git::git;

use super::testing::{Mirrored, OLD_NEWS};
use super::*;
use crate::testing::{cpython, sha_of};

/// The rows as a reviewer reads them: old and new line numbers, a marker, and the text.
fn drawn(diff: &Diff) -> String {
    let mut out = String::new();
    for row in diff.rows(0..diff.row_count()) {
        let line = match row {
            DiffRow::File {
                path,
                old_path,
                change,
            } => {
                let file = diff.files().iter().find(|file| file.path == *path).unwrap();
                assert_eq!((&file.old_path, file.change), (old_path, *change));
                let from = old_path
                    .as_ref()
                    .map(|old| format!("{old} → "))
                    .unwrap_or_default();
                let (row, columns) = (file.row, file.columns);
                format!("=== {from}{path} {change:?} at row {row}, {columns} wide")
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
}

#[test]
fn a_mirror_holds_a_diff_only_with_both_the_head_and_the_target_branch() {
    let mirrored = Mirrored::new("held");
    let elsewhere = sha_of('1');

    assert_eq!(held(&mirrored.mirror, "main", &mirrored.head), Held::Both);
    assert_eq!(
        held(&mirrored.mirror, "3.14", &mirrored.head),
        Held::NoTarget
    );
    assert_eq!(held(&mirrored.mirror, "main", &elsewhere), Held::NoHead);
}

#[test]
fn a_diff_git_cant_read_says_so_in_plain_words() {
    let mirrored = Mirrored::new("unreadable");

    let problem = read(
        &mirrored.mirror,
        "3.14",
        &Start::TargetBranch,
        &mirrored.head,
    )
    .unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Internal);
    assert_eq!(
        problem.message,
        "Tsugiki couldn't read this PR's diff from its mirror."
    );
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
}

#[test]
fn the_diff_on_show_answers_each_chunk_and_another_head_is_read_again() {
    let mirrored = Mirrored::new("on-show");
    let diffs = Diffs::default();
    let pr = PrAt {
        repository: cpython(),
        number: 7,
        head: mirrored.head.clone(),
    };
    let moved = PrAt {
        head: sha_of('2'),
        ..pr.clone()
    };
    let other_pr = PrAt {
        number: 8,
        ..pr.clone()
    };

    let main = Against::Branch("main", &Start::TargetBranch);

    let first = diffs
        .get_or_read(&pr, main, || Ok(mirrored.read()))
        .unwrap();
    let again = diffs
        .get_or_read(&pr, main, || panic!("read again"))
        .unwrap();
    let from_base = Against::Base(&first.base);
    let chunk = diffs
        .get_or_read(&pr, from_base, || panic!("read again"))
        .unwrap();
    let other = diffs
        .get_or_read(&moved, main, || Ok(mirrored.read()))
        .unwrap();

    assert!(Arc::ptr_eq(&first, &again));
    assert!(Arc::ptr_eq(&first, &chunk));
    assert_eq!(other.pr.head, moved.head);
    let no = || Err(Error::new(ErrorKind::Internal, "no"));
    assert!(diffs.get_or_read(&other_pr, main, no).is_err());
}

#[test]
fn a_diff_against_another_branch_or_from_another_base_is_read_again() {
    let mirrored = Mirrored::new("retargeted");
    let diffs = Diffs::default();
    let pr = PrAt {
        repository: cpython(),
        number: 7,
        head: mirrored.head.clone(),
    };
    let head = &mirrored.head;
    let main = Against::Branch("main", &Start::TargetBranch);
    let first = diffs
        .get_or_read(&pr, main, || Ok(mirrored.read()))
        .unwrap();
    let elsewhere = Against::Base(head);

    let retargeted = diffs.get_or_read(&pr, Against::Branch("3.14", &Start::TargetBranch), || {
        Ok(mirrored.read())
    });
    let retargeted = retargeted.unwrap();
    // A chunk of a diff the window holds from another base, read while the PR targets main.
    let from_base = diffs.get_or_read(&pr, elsewhere, || {
        read(&mirrored.mirror, "main", &Start::Base(head.clone()), head)
    });

    assert!(!Arc::ptr_eq(&first, &retargeted));
    assert_eq!(from_base.unwrap().diff.row_count(), 0);
    assert!(diffs.shown(&pr, elsewhere).is_some());
    // Its base may not be main's, so it can't answer for main.
    assert!(diffs.shown(&pr, main).is_none());
}

#[test]
fn a_diff_from_a_base_reads_from_that_base_whatever_the_branch_says() {
    let mirrored = Mirrored::new("from-base");
    let first = mirrored.read();
    let _ = git(
        &mirrored.mirror,
        &["branch", "3.14", mirrored.head.as_str()],
    );

    let again = read(
        &mirrored.mirror,
        "3.14",
        &Start::Base(first.base.clone()),
        &mirrored.head,
    )
    .unwrap();
    let retargeted = read(
        &mirrored.mirror,
        "3.14",
        &Start::TargetBranch,
        &mirrored.head,
    )
    .unwrap();

    assert_eq!(again.base, first.base);
    assert_eq!(again.diff.rows(0..100), first.diff.rows(0..100));
    assert_eq!(retargeted.base, mirrored.head);
    assert_eq!(retargeted.diff.row_count(), 0);
}

#[test]
fn a_base_the_mirror_lacks_cant_be_read() {
    let mirrored = Mirrored::new("no-base");
    let elsewhere = sha_of('2');

    let problem = read(
        &mirrored.mirror,
        "main",
        &Start::Base(elsewhere.clone()),
        &mirrored.head,
    );

    assert_eq!(problem.unwrap_err().kind, ErrorKind::Internal);
}

#[test]
fn a_read_that_ends_after_a_later_one_leaves_the_later_on_show() {
    let diffs = Diffs::default();
    let pr = |number| PrAt {
        repository: cpython(),
        number,
        head: sha_of('1'),
    };
    let empty = || {
        Ok(MirrorDiff {
            base: sha_of('0'),
            diff: Diff::parse("", "")?,
            news: Vec::new(),
        })
    };

    let main = Against::Branch("main", &Start::TargetBranch);

    // The user opens PR 8 while PR 7's diff is still being read.
    let earlier = diffs.get_or_read(&pr(7), main, || {
        let later = diffs.get_or_read(&pr(8), main, empty).unwrap();
        assert_eq!(later.pr.number, 8);
        empty()
    });

    assert_eq!(earlier.unwrap().pr.number, 7);
    assert!(diffs.shown(&pr(8), main).is_some());
    assert!(diffs.shown(&pr(7), main).is_none());
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
fn each_file_names_the_lines_its_hunks_leave_out() {
    let numstat = "1\t1\ta.py\x001\t1\tb.py\x001\t1\tc.py\x00";
    let a = "diff --git a/a.py b/a.py\n\
             @@ -8,7 +8,7 @@ def f():\n l8\n l9\n l10\n-l11\n+L11\n l12\n l13\n l14\n\
             @@ -40,6 +40,7 @@ def g():\n l40\n l41\n l42\n+new\n l43\n l44\n l45\n";
    // The change ends one line before the end of the file, so nothing follows it.
    let b = "diff --git a/b.py b/b.py\n@@ -1,2 +1,2 @@\n-a\n+A\n b\n";
    // Three lines of context, and then the file ends without a newline.
    let c = "diff --git a/c.py b/c.py\n@@ -1,4 +1,4 @@\n-a\n+A\n b\n c\n d\n\\ No newline at end of file\n";

    let diff = Diff::parse(numstat, &format!("{a}{b}{c}")).unwrap();

    let [a, b, c] = diff.files() else {
        panic!("three files");
    };
    let above = |row, line, lines| Gap {
        row,
        old: line,
        new: line,
        lines,
    };
    assert_eq!(a.gaps, [above(1, 1, 7), above(10, 15, 25)]);
    assert_eq!(a.rest, Some(Rest { old: 46, new: 47 }));
    assert_eq!((b.gaps.as_slice(), b.rest), ([].as_slice(), None));
    assert_eq!((c.gaps.as_slice(), c.rest), ([].as_slice(), None));
}

#[test]
fn each_file_names_its_changes_so_side_by_side_can_pair_their_removed_and_added_rows() {
    let numstat = "4\t5\ta.py\x001\t2\tb.py\x00";
    // Rows 2 to 4 pair two removed lines with one added; row 6 is removed alone, and row 8 added alone;
    // rows 10 to 12 pair one with two.
    let a =
        "diff --git a/a.py b/a.py\n@@ -1,8 +1,7 @@\n-a\n-b\n+B\n c\n-d\n e\n+f\n g\n-h\n+H\n+I\n";
    // Each side ends with the note about its last line.
    let b = "diff --git a/b.py b/b.py\n@@ -1 +1,2 @@\n-a\n\\ No newline at end of file\n+a\n+b\n\
             \\ No newline at end of file\n";

    let diff = Diff::parse(numstat, &format!("{a}{b}")).unwrap();

    let [a, b] = diff.files() else {
        panic!("two files");
    };
    let pair = |row, removed, added| Pair {
        row,
        removed,
        added,
    };
    assert_eq!(
        a.pairs,
        [pair(2, 2, 1), pair(6, 1, 0), pair(8, 0, 1), pair(10, 1, 2)]
    );
    assert_eq!(b.pairs, [pair(15, 2, 3)]);
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

#[test]
fn each_row_is_highlighted_from_its_version_and_a_removed_line_from_the_merge_base() {
    use crate::highlight::tests::with_spans;

    let mirrored = Mirrored::new("spans");
    let pr = PrAt {
        repository: cpython(),
        number: 7,
        head: mirrored.head.clone(),
    };
    let open = Diffs::default()
        .get_or_read(&pr, Against::Branch("main", &Start::TargetBranch), || {
            Ok(mirrored.read())
        })
        .unwrap();
    let all = 0..open.diff.row_count();
    // main marks Lib/ast.py generated, and the window has collapsed no file.
    let highlighter = Highlighter::start();
    let rows = |rows| Chunk {
        rows,
        collapsed: &[],
    };
    let spans = highlighter.spans(&open, &mirrored.mirror, rows(all.clone()));
    let chunk = highlighter.spans(&open, &mirrored.mirror, rows(3..9));
    // A highlighter of its own, as this one keeps each version it parsed.
    let unreadable = Highlighter::start().spans(&open, Path::new("no-mirror"), rows(all.clone()));

    let drawn: Vec<String> = (open.diff.rows(all).iter().zip(&spans))
        .filter_map(|(row, spans)| match row {
            DiffRow::Context { text, .. } => Some(format!(" {}", with_spans(text, spans))),
            DiffRow::Added { text, .. } => Some(format!("+{}", with_spans(text, spans))),
            DiffRow::Removed { text, .. } => Some(format!("-{}", with_spans(text, spans))),
            DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => None,
        })
        .collect();
    insta::assert_snapshot!(drawn.join("\n"));
    assert_eq!(chunk, spans.get(3..9).unwrap());
    assert!(unreadable.iter().all(Vec::is_empty));
}

#[test]
fn only_the_target_branch_s_attributes_count_and_an_index_a_run_left_goes() {
    let mirrored = Mirrored::new("own-attributes");
    let own = mirrored.root.join("own.attributes");
    fs::write(&own, "Modules/** linguist-generated\n").unwrap();
    let _ = git(
        &mirrored.mirror,
        &["config", "core.attributesFile", &own.to_string_lossy()],
    );
    let left = mirrored.mirror.join("tsugiki-attributes-1-0.index");
    fs::write(&left, "").unwrap();

    let diff = mirrored.diff();

    let generated = diff.files().iter().filter(|file| file.generated);
    let generated: Vec<&str> = generated.map(|file| file.path.as_str()).collect();
    assert_eq!(generated, ["Lib/ast.py", "logo.png"]);
    assert!(!left.exists());
    let indexes = fs::read_dir(&mirrored.mirror).unwrap().flatten();
    assert!(
        indexes
            .into_iter()
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(INDEX))
    );
}

#[test]
fn a_left_index_the_sweep_cant_remove_stays_and_the_diff_still_reads() {
    let mirrored = Mirrored::new("stuck-index");
    let stuck = mirrored.mirror.join("tsugiki-attributes-1-0.index");
    fs::create_dir(&stuck).unwrap();

    let diff = mirrored.diff();

    assert!(diff.files().iter().any(|file| file.generated));
    assert!(stuck.is_dir());
}

#[test]
fn attributes_git_cant_read_mark_nothing_generated() {
    let mirrored = Mirrored::new("no-attributes");
    let mut diff = mirrored.diff();

    let made = TargetIndex::new(&mirrored.mirror).make("no-such-branch");
    diff.mark_generated(made.map(|()| vec![true; 7]), &mirrored.mirror);

    assert!(diff.files().iter().all(|file| !file.generated));
}

#[test]
fn a_prs_news_entries_come_rendered_from_its_head() {
    let entry = "Misc/NEWS.d/next/Library/2026-11-07-10-00-00.gh-issue-1.New.rst";
    let mirrored = Mirrored::with("news", |work| {
        fs::write(work.join(entry), "Fix :func:`ast.parse`.\n").unwrap();
        fs::write(work.join("Misc/NEWS.d/3.14.0.rst"), "Released.\n").unwrap();
        let _ = git(work, &["rm", "-q", OLD_NEWS]);
    });

    let read = mirrored.read();

    assert_eq!(
        read.news,
        [NewsEntry::render(entry, "Fix :func:`ast.parse`.\n")]
    );
}

#[test]
fn a_news_entry_whose_path_has_a_line_break_leaves_the_others_their_text() {
    let first = "Misc/NEWS.d/next/Library/2026-11-07-10-00-00.gh-issue-1.First.rst";
    let last = "Misc/NEWS.d/next/Library/2026-11-07-11-00-00.gh-issue-3.Last.rst";
    let mirrored = Mirrored::with("news-line-break", |work| {
        fs::write(work.join(first), "First.\n").unwrap();
        fs::write(work.join(last), "Last.\n").unwrap();
    });
    let read = mirrored.read();
    let entry = |path: &str| {
        (read.diff.files().iter())
            .find(|file| file.path == path)
            .unwrap()
            .clone()
    };
    let mut broken = entry(first);
    // Its second line names the first entry, which git would read in the place of the last.
    broken.path = format!(
        "Misc/NEWS.d/next/Library/2.gh-issue-2.rst\n{}:{first}",
        mirrored.head.as_str()
    );
    let files = [entry(first), broken, entry(last)];

    let entries = news(&mirrored.mirror, &mirrored.head, &files);

    assert_eq!(
        entries,
        [
            NewsEntry::render(first, "First.\n"),
            NewsEntry::render(last, "Last.\n"),
        ]
    );
}

#[test]
fn a_pr_without_news_entries_has_none() {
    let mirrored = Mirrored::new("no-news");

    assert!(mirrored.read().news.is_empty());
}
