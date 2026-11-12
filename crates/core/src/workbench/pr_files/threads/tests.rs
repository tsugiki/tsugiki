use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::{Comment, HiddenReason};
use crate::github::pull_request_content::{Opening, Submission};
use crate::highlight::Highlight;

/// `Lib/ast.py` with one line changed, and `Lib/old.py` renamed to `Lib/new.py` with a line added.
///
/// | Row | What |
/// |---|---|
/// | 0, 1 | `Lib/ast.py`'s header and hunk |
/// | 2 | line 1 at both |
/// | 3, 4 | line 2 removed, line 2 added |
/// | 5 | line 3 at both |
/// | 6, 7 | `Lib/new.py`'s header and hunk |
/// | 8 | line 4 at both |
/// | 9 | line 5 added |
/// | 10 | line 5 at the base, 6 at the head |
fn diff() -> Diff {
    let numstat = "1\t1\tLib/ast.py\0\
                   1\t0\t\0Lib/old.py\0Lib/new.py\0";
    let patch = "diff --git a/Lib/ast.py b/Lib/ast.py\n\
                 @@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n\
                 diff --git a/Lib/old.py b/Lib/new.py\n\
                 @@ -4,2 +4,3 @@\n d\n+e\n f\n";
    Diff::parse(numstat, patch).unwrap()
}

/// `Lib/re.py`, 30 lines long, with line 10 changed: lines 1 to 6 are a gap above the hunk, and lines
/// from 14 on the rest of the file.
///
/// | Row | What |
/// |---|---|
/// | 0, 1 | its header and hunk |
/// | 2 to 4 | lines 7 to 9 at both |
/// | 5, 6 | line 10 removed, line 10 added |
/// | 7 to 9 | lines 11 to 13 at both |
fn long() -> Diff {
    let numstat = "1\t1\tLib/re.py\0";
    let patch = "diff --git a/Lib/re.py b/Lib/re.py\n\
                 @@ -7,7 +7,7 @@\n g\n h\n i\n-j\n+J\n k\n l\n m\n";
    Diff::parse(numstat, patch).unwrap()
}

fn comment(hidden: Option<HiddenReason>) -> ThreadComment {
    ThreadComment {
        id: None,
        comment: Comment {
            author: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            body_html: "<p>Why?</p>".to_owned(),
            hidden,
        },
        submission: Submission::Submitted,
    }
}

/// An open thread `id` on line `line` of `path`, on `side`, with one comment.
fn thread(id: &str, path: &str, side: Side, line: u32) -> ThreadLine {
    ThreadLine {
        id: ThreadId::new(id),
        path: path.to_owned(),
        opening: Opening::Words {
            words: "Why?".to_owned(),
        },
        replies: 0,
        resolution: Resolution::Open,
        anchor: Anchor::Current,
        subject: Subject::Line {
            side,
            line: Some(line),
            original: Some(line),
        },
        comments: vec![comment(None)],
        hunk: DiffHunk::parse("@@ -1,2 +1,2 @@\n x = 1\n-y = 2\n+y = 3"),
    }
}

/// `thread` as GitHub marks it once the code under it has changed.
fn outdated(id: &str, path: &str) -> ThreadLine {
    let mut moved = thread(id, path, Side::Right, 1);
    moved.anchor = Anchor::Outdated;
    moved.subject = Subject::Line {
        side: Side::Right,
        line: None,
        original: Some(2),
    };
    moved
}

/// `thread` on the whole of `path`.
fn on_the_file(id: &str, path: &str) -> ThreadLine {
    let mut whole = thread(id, path, Side::Right, 1);
    whole.subject = Subject::File;
    whole.hunk = None;
    whole
}

fn placed(diff: &Diff, threads: &[ThreadLine]) -> Vec<DiffThread> {
    place(diff, threads.iter())
}

/// Each thread's id, and where it is: `row N`, `file N`, `outdated N`, `outside N:line@head`, or
/// `gone path`.
fn places(placed: &[DiffThread]) -> Vec<(&str, String)> {
    let at = |thread: &DiffThread| match &thread.at {
        Placement::Row { row } => format!("row {row}"),
        Placement::File { file } => format!("file {file}"),
        Placement::Outdated { file, .. } => format!("outdated {file}"),
        Placement::Outside {
            file,
            line,
            head_line,
        } => format!("outside {file}:{line}@{head_line}"),
        Placement::Gone { path, .. } => format!("gone {path}"),
    };
    placed
        .iter()
        .map(|thread| (thread.id.as_str(), at(thread)))
        .collect()
}

#[test]
fn a_thread_sits_under_its_line_at_the_head_or_at_the_merge_base_in_row_order() {
    let threads = [
        thread("added", "Lib/ast.py", Side::Right, 2),
        thread("removed", "Lib/ast.py", Side::Left, 2),
        thread("context at the head", "Lib/new.py", Side::Right, 6),
        thread("context at the base", "Lib/ast.py", Side::Left, 3),
    ];

    let placed = placed(&diff(), &threads);

    assert_eq!(
        places(&placed),
        [
            ("removed", "row 3".to_owned()),
            ("added", "row 4".to_owned()),
            ("context at the base", "row 5".to_owned()),
            ("context at the head", "row 10".to_owned())
        ]
    );
}

#[test]
fn a_renamed_file_s_threads_name_it_either_way() {
    let threads = [
        thread("new name", "Lib/new.py", Side::Right, 5),
        thread("old name", "Lib/old.py", Side::Left, 4),
    ];

    let placed = placed(&diff(), &threads);

    assert_eq!(
        places(&placed),
        [
            ("old name", "row 8".to_owned()),
            ("new name", "row 9".to_owned())
        ]
    );
}

#[test]
fn a_thread_on_a_file_that_took_a_renamed_file_s_old_name_stays_on_it() {
    // `Lib/a.py` moved to `Lib/b.py`, and a new `Lib/a.py` came after it.
    let numstat = "1\t0\t\0Lib/a.py\0Lib/b.py\0\
                   1\t0\tLib/a.py\0";
    let patch = "diff --git a/Lib/a.py b/Lib/b.py\n\
                 @@ -1 +1,2 @@\n x\n+y\n\
                 diff --git a/Lib/a.py b/Lib/a.py\n\
                 @@ -0,0 +1,2 @@\n+new\n+file\n";
    let diff = Diff::parse(numstat, patch).unwrap();
    let threads = [thread("new file", "Lib/a.py", Side::Right, 2)];

    let placed = placed(&diff, &threads);

    // The rename's rows are 0 to 3, and the new file's 4 to 7.
    assert_eq!(places(&placed), [("new file", "row 7".to_owned())]);
}

#[test]
fn each_file_shows_its_threads_on_the_file_then_its_outdated_ones_then_those_on_its_lines() {
    let threads = [
        thread("line", "Lib/new.py", Side::Right, 5),
        outdated("outdated", "Lib/new.py"),
        on_the_file("whole", "Lib/new.py"),
        outdated("earlier file", "Lib/ast.py"),
        thread("gone", "Lib/re.py", Side::Right, 1),
        on_the_file("also gone", "Lib/gone.py"),
    ];

    let placed = placed(&diff(), &threads);

    assert_eq!(
        places(&placed),
        [
            ("earlier file", "outdated 0".to_owned()),
            ("whole", "file 1".to_owned()),
            ("outdated", "outdated 1".to_owned()),
            ("line", "row 9".to_owned()),
            ("also gone", "gone Lib/gone.py".to_owned()),
            ("gone", "gone Lib/re.py".to_owned()),
        ],
        "the threads on files the diff no longer holds come last, by file"
    );
}

#[test]
fn a_thread_on_a_line_no_hunk_shows_sits_in_its_gap_or_after_the_file_s_last_row() {
    let threads = [
        thread("after", "Lib/re.py", Side::Right, 20),
        thread("in the gap at the base", "Lib/re.py", Side::Left, 3),
        thread("in the hunk", "Lib/re.py", Side::Right, 12),
        thread("further down the gap", "Lib/re.py", Side::Right, 5),
    ];

    let placed = placed(&long(), &threads);

    assert_eq!(
        places(&placed),
        [
            ("in the gap at the base", "outside 0:3@3".to_owned()),
            ("further down the gap", "outside 0:5@5".to_owned()),
            ("in the hunk", "row 8".to_owned()),
            ("after", "outside 0:20@20".to_owned()),
        ]
    );
}

#[test]
fn a_line_github_places_that_the_file_doesn_t_have_counts_as_outdated() {
    // Lib/ast.py's hunk ends its file, and an added line has no number at the base.
    let threads = [
        thread("past the end", "Lib/ast.py", Side::Right, 9),
        thread("an added line at the base", "Lib/new.py", Side::Left, 6),
    ];

    let placed = placed(&diff(), &threads);

    assert_eq!(
        places(&placed),
        [
            ("past the end", "outdated 0".to_owned()),
            ("an added line at the base", "outdated 1".to_owned())
        ]
    );
}

#[test]
fn an_outdated_thread_shows_its_hunk_highlighted_ending_at_its_line() {
    let mut moved = outdated("outdated", "Lib/new.py");
    moved.hunk = DiffHunk::parse(
        "@@ -10,3 +10,3 @@ def f():\n x = 1\n-y = 2\n+y = 3\n\\ No newline at end of file",
    );
    let mut gone = outdated("gone", "Doc/notes.txt");
    gone.hunk = DiffHunk::parse("@@ -1 +1 @@\n-old\n+new");

    let placed = placed(&diff(), &[moved, gone]);

    // The code, as the window reads it.
    let code = serde_json::to_value(&placed).unwrap()[0]["at"]["code"].clone();
    let gone = serde_json::to_value(&placed).unwrap()[1]["at"]["code"].clone();
    let text = |text: &str| text.to_owned();
    let rows = serde_json::to_value([
        CodeRow::Hunk {
            text: text("@@ -10,3 +10,3 @@ def f():"),
        },
        CodeRow::Context {
            old: 10,
            new: 10,
            text: text("x = 1"),
        },
        CodeRow::Removed {
            old: 11,
            text: text("y = 2"),
        },
        CodeRow::Added {
            new: 11,
            text: text("y = 3"),
        },
        CodeRow::Note {
            text: text("\\ No newline at end of file"),
        },
    ])
    .unwrap();
    assert_eq!(code["rows"], rows);
    let painted = |code: &serde_json::Value, row: usize| {
        let spans = code["spans"][row].as_array().cloned().unwrap_or_default();
        let painted = spans.iter().map(|span| span["highlight"].clone());
        painted.collect::<Vec<_>>()
    };
    assert_eq!(code["spans"].as_array().map(Vec::len), Some(5));
    assert_eq!((painted(&code, 0), painted(&code, 4)), (vec![], vec![]));
    let constant = serde_json::to_value(Highlight::Constant).unwrap();
    assert!(painted(&code, 3).contains(&constant), "{code}");
    assert_eq!(
        gone["spans"],
        serde_json::json!([[], [], []]),
        "plain text has no spans"
    );
}
#[test]
fn a_thread_of_only_hidden_comments_counts_as_resolved() {
    let mut hidden = thread("hidden", "Lib/ast.py", Side::Right, 1);
    hidden.comments = vec![comment(Some(HiddenReason::Spam))];
    let mut partly = thread("partly", "Lib/ast.py", Side::Right, 3);
    partly.comments = vec![comment(Some(HiddenReason::Spam)), comment(None)];
    partly.replies = 4;

    let placed = placed(&diff(), &[hidden, partly]);

    let marks: Vec<(Resolution, u32)> = placed
        .iter()
        .map(|thread| (thread.resolution, thread.later_comments))
        .collect();
    assert_eq!(marks, [(Resolution::Resolved, 0), (Resolution::Open, 3)]);
}
