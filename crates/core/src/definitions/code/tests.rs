use std::fs;
use std::sync::Barrier;
use std::thread;

use super::*;
use crate::definitions::testing::Fixture;
use crate::github::CommitSha;
use crate::highlight::Highlight;
use crate::testing::temp_path;

fn fixture(name: &str) -> Fixture {
    Fixture::new(
        name,
        &[("a.py", "one\r\ntwo\nthree\n")],
        &[("a.py", Some("one\nTWO\nthree\nfour\n"))],
    )
}

fn at(commit: &CommitSha) -> HitFile {
    HitFile::Repository {
        path: "a.py".to_owned(),
        commit: commit.clone(),
    }
}

fn of(file: &HitFile, lines: Range<u32>) -> HitLines {
    HitLines {
        file: file.clone(),
        lines,
    }
}

fn code(count: u32, lines: &[&str]) -> Code {
    Code {
        count,
        lines: lines.iter().map(|line| (*line).to_owned()).collect(),
    }
}

#[test]
fn a_file_of_the_repository_is_read_at_the_hit_s_commit_without_line_endings() {
    let fixture = fixture("definitions-code-commits");
    let files = HitFiles::default();
    let at_base = at(&fixture.checkout.at.base);
    let at_head = at(&fixture.checkout.at.head);

    let base = files.read(&fixture.mirror, &of(&at_base, 1..10));
    let head = files.read(&fixture.mirror, &of(&at_head, 2..4));
    let base_again = files.read(&fixture.mirror, &of(&at_base, 3..4));

    assert_eq!(base, Ok(code(3, &["one", "two", "three"])));
    assert_eq!(head, Ok(code(4, &["TWO", "three"])));
    assert_eq!(base_again, Ok(code(3, &["three"])));
}

#[test]
fn the_next_chunk_of_the_same_file_reads_it_once() {
    let fixture = fixture("definitions-code-once");
    let files = HitFiles::default();
    let at_head = at(&fixture.checkout.at.head);

    let first = files.read(&fixture.mirror, &of(&at_head, 1..3));
    // Without its Mirror, the file comes from what the first read kept.
    fs::remove_dir_all(fixture.mirror.parent().unwrap()).unwrap();
    let next = files.read(&fixture.mirror, &of(&at_head, 3..5));

    assert_eq!(first, Ok(code(4, &["one", "TWO"])));
    assert_eq!(next, Ok(code(4, &["three", "four"])));
    assert_eq!(files.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn a_first_chunk_s_lines_and_spans_asked_together_share_one_read() {
    let fixture = fixture("definitions-code-together");
    let files = HitFiles::default();
    let highlighter = Highlighter::start();
    let wanted = of(&at(&fixture.checkout.at.head), 1..201);
    let both = Barrier::new(2);

    let (lines, spans) = thread::scope(|scope| {
        let lines = scope.spawn(|| {
            let _ready = both.wait();
            files.read(&fixture.mirror, &wanted)
        });
        let _ready = both.wait();
        let spans = files.spans(&highlighter, &fixture.mirror, &wanted);
        (lines.join().unwrap(), spans)
    });

    assert_eq!(lines.map(|code| code.count), Ok(4));
    assert_eq!(spans.map(|spans| spans.len()), Ok(200));
    assert_eq!(files.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn a_line_ends_at_a_lone_carriage_return_too_as_lsp_counts_lines() {
    let lines = split("one\rtwo\r\nthree\n\nfive");

    assert_eq!(lines, ["one", "two", "three", "", "five"]);
    assert_eq!(split("one\n"), ["one"]);
    assert!(split("").is_empty());
}

#[test]
fn lines_past_the_end_don_t_come_and_a_read_hands_at_most_two_thousand() {
    let text: Vec<String> = (1..=2500).map(|n| n.to_string()).collect();

    let past = some_lines(&text, 2499..2600);
    let many = some_lines(&text, 0..u32::MAX);

    assert_eq!(past, code(2500, &["2499", "2500"]));
    assert_eq!(many.count, 2500);
    assert_eq!(many.lines.len(), 2000);
    assert_eq!(many.lines.first().map(String::as_str), Some("1"));
}

#[test]
fn a_file_the_commit_doesn_t_hold_says_so_each_time_it_s_asked_and_so_does_a_missing_mirror() {
    let fixture = fixture("definitions-code-missing");
    let commit = fixture.checkout.at.base.clone();
    let missing = HitFile::Repository {
        path: "b.py".to_owned(),
        commit: commit.clone(),
    };
    let files = HitFiles::default();

    let read = files.read(&fixture.mirror, &of(&missing, 1..10));
    // A read that failed is forgotten, so the next chunk asks git again.
    let again = files.read(&fixture.mirror, &of(&missing, 10..20));
    let reads = files.reads.load(Ordering::Relaxed);
    let no_mirror = files.read(&fixture.mirror.join("none"), &of(&at(&commit), 1..10));

    let short = commit.as_str().get(..7).unwrap();
    let message = format!("Tsugiki's copy of the Repository has no b.py at {short}.");
    assert_eq!(read, Err(Error::new(ErrorKind::NotFound, &message)));
    assert_eq!((again, reads), (read, 2));
    let none = "Tsugiki has no copy of this Repository yet.";
    assert_eq!(no_mirror, Err(Error::new(ErrorKind::NotFound, none)));
}

#[test]
fn a_file_outside_the_repository_is_read_from_disk_once_a_server_named_it() {
    let folder = temp_path("definitions-code-outside");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("builtins.pyi");
    fs::write(&path, "def print(\n    *values: object,\n): ...\n").unwrap();
    let files = HitFiles::default();
    let file = HitFile::Outside { path: path.clone() };

    let unnamed = files.read(&folder, &of(&file, 1..2));
    files.name(path.clone());
    let read = files.read(&folder, &of(&file, 1..2));

    let message = format!("No language server named {}.", path.display());
    assert_eq!(unnamed, Err(Error::new(ErrorKind::NotFound, &message)));
    assert_eq!(read, Ok(code(3, &["def print("])));
}

#[test]
fn a_named_file_that_has_gone_from_disk_says_so() {
    let path = temp_path("definitions-code-gone").join("builtins.pyi");
    let files = HitFiles::default();
    files.name(path.clone());

    let file = HitFile::Outside { path: path.clone() };
    let read = files.read(Path::new(""), &of(&file, 1..2));

    let message = read.unwrap_err().message;
    let start = format!("Tsugiki can't read {}: ", path.display());
    assert!(message.starts_with(&start), "{message}");
}

fn span(start: u32, end: u32, highlight: Highlight) -> Span {
    Span {
        start,
        end,
        highlight,
    }
}

#[test]
fn a_file_at_a_commit_read_from_the_mirror_is_highlighted_line_by_line_as_lsp_counts_lines() {
    // A lone `\r` ends the first line, as it does for the lines `read` gives.
    let python = "x = 1\rdef f():\r\n    return 'two'\n";
    let fixture = Fixture::new("definitions-code-spans", &[("a.py", python)], &[]);
    let files = HitFiles::default();
    let highlighter = Highlighter::start();
    let at_base = at(&fixture.checkout.at.base);

    let spans = files.spans(&highlighter, &fixture.mirror, &of(&at_base, 2..10));
    let lines = files.read(&fixture.mirror, &of(&at_base, 2..10));

    let def = vec![
        span(0, 3, Highlight::Keyword),
        span(4, 5, Highlight::Function),
    ];
    let return_two = vec![
        span(4, 10, Highlight::Keyword),
        span(11, 16, Highlight::String),
    ];
    // The lines past the end have none.
    let past = vec![Vec::new(); 6];
    assert_eq!(spans, Ok([vec![def, return_two], past].concat()));
    assert_eq!(lines, Ok(code(3, &["def f():", "    return 'two'"])));
}

#[test]
fn the_highlighter_keeps_a_file_s_tree_so_its_next_chunk_isn_t_read_again() {
    let fixture = Fixture::new(
        "definitions-code-kept",
        &[("a.py", "def f():\n    pass\n"), ("b.py", "x = 1\n")],
        &[],
    );
    let files = HitFiles::default();
    let highlighter = Highlighter::start();
    let base = &fixture.checkout.at.base;
    let b = HitFile::Repository {
        path: "b.py".to_owned(),
        commit: base.clone(),
    };

    let first = files.spans(&highlighter, &fixture.mirror, &of(&at(base), 1..2));
    // Another file is the last read now, and without the Mirror neither can be read again.
    drop(files.read(&fixture.mirror, &of(&b, 1..2)));
    fs::remove_dir_all(fixture.mirror.parent().unwrap()).unwrap();
    let next = files.spans(&highlighter, &fixture.mirror, &of(&at(base), 2..3));

    let def = vec![
        span(0, 3, Highlight::Keyword),
        span(4, 5, Highlight::Function),
    ];
    assert_eq!(first, Ok(vec![def]));
    assert_eq!(next, Ok(vec![vec![span(4, 8, Highlight::Keyword)]]));
}

#[test]
fn a_file_outside_the_repository_is_highlighted_once_a_server_named_it() {
    let folder = temp_path("definitions-code-outside-spans");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("builtins.pyi");
    fs::write(&path, "def print(): ...\n").unwrap();
    let files = HitFiles::default();
    let highlighter = Highlighter::start();
    let file = HitFile::Outside { path: path.clone() };

    let unnamed = files.spans(&highlighter, &folder, &of(&file, 1..2));
    files.name(path.clone());
    let spans = files.spans(&highlighter, &folder, &of(&file, 1..2));

    let message = format!("No language server named {}.", path.display());
    assert_eq!(unnamed, Err(Error::new(ErrorKind::NotFound, &message)));
    let def = vec![
        span(0, 3, Highlight::Keyword),
        span(4, 9, Highlight::Function),
    ];
    assert_eq!(spans, Ok(vec![def]));
}

#[test]
fn a_file_outside_the_repository_that_changes_on_disk_is_highlighted_as_its_lines_read_now() {
    let folder = temp_path("definitions-code-outside-changed");
    fs::create_dir_all(&folder).unwrap();
    let (path, other) = (folder.join("builtins.pyi"), folder.join("other.pyi"));
    fs::write(&path, "def print(): ...\n").unwrap();
    fs::write(&other, "x = 1\n").unwrap();
    let files = HitFiles::default();
    files.name(path.clone());
    files.name(other.clone());
    let highlighter = Highlighter::start();
    let file = of(&HitFile::Outside { path: path.clone() }, 1..2);

    let before = files.spans(&highlighter, &folder, &file);
    fs::write(&path, "x = 'print'\n").unwrap();
    // Until another file is read, the lines and the spans stay those read before.
    let kept = files.spans(&highlighter, &folder, &file);
    drop(files.read(&folder, &of(&HitFile::Outside { path: other }, 1..2)));
    let lines = files.read(&folder, &file);
    let after = files.spans(&highlighter, &folder, &file);

    let def = vec![
        span(0, 3, Highlight::Keyword),
        span(4, 9, Highlight::Function),
    ];
    assert_eq!(kept, before);
    assert_eq!(before, Ok(vec![def]));
    assert_eq!(lines, Ok(code(1, &["x = 'print'"])));
    let assigned = vec![
        span(2, 3, Highlight::Keyword),
        span(4, 11, Highlight::String),
    ];
    assert_eq!(after, Ok(vec![assigned]));
}

#[test]
fn a_file_that_can_t_be_read_has_no_spans_and_says_why() {
    let fixture = fixture("definitions-code-spans-missing");
    let commit = fixture.checkout.at.base.clone();
    let missing = HitFile::Repository {
        path: "b.py".to_owned(),
        commit: commit.clone(),
    };

    let highlighter = Highlighter::start();
    let spans = HitFiles::default().spans(&highlighter, &fixture.mirror, &of(&missing, 1..2));

    let short = commit.as_str().get(..7).unwrap();
    let message = format!("Tsugiki's copy of the Repository has no b.py at {short}.");
    assert_eq!(spans, Err(Error::new(ErrorKind::NotFound, &message)));
}
