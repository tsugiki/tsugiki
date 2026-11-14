use std::fs;

use super::*;
use crate::definitions::testing::Fixture;
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

    let base = files.read(&fixture.mirror, &at_base, 1..10);
    let head = files.read(&fixture.mirror, &at_head, 2..4);
    let base_again = files.read(&fixture.mirror, &at_base, 3..4);

    assert_eq!(base, Ok(code(3, &["one", "two", "three"])));
    assert_eq!(head, Ok(code(4, &["TWO", "three"])));
    assert_eq!(base_again, Ok(code(3, &["three"])));
}

#[test]
fn the_next_chunk_of_the_same_file_reads_it_once() {
    let fixture = fixture("definitions-code-once");
    let files = HitFiles::default();
    let at_head = at(&fixture.checkout.at.head);

    let first = files.read(&fixture.mirror, &at_head, 1..3);
    // Without its Mirror, the file comes from what the first read kept.
    fs::remove_dir_all(fixture.mirror.parent().unwrap()).unwrap();
    let next = files.read(&fixture.mirror, &at_head, 3..5);

    assert_eq!(first, Ok(code(4, &["one", "TWO"])));
    assert_eq!(next, Ok(code(4, &["three", "four"])));
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
fn a_file_the_commit_doesn_t_hold_says_so_and_so_does_a_missing_mirror() {
    let fixture = fixture("definitions-code-missing");
    let commit = fixture.checkout.at.base.clone();
    let missing = HitFile::Repository {
        path: "b.py".to_owned(),
        commit: commit.clone(),
    };
    let files = HitFiles::default();

    let read = files.read(&fixture.mirror, &missing, 1..10);
    let no_mirror = files.read(&fixture.mirror.join("none"), &at(&commit), 1..10);

    let short = commit.as_str().get(..7).unwrap();
    let message = format!("Tsugiki's copy of the Repository has no b.py at {short}.");
    assert_eq!(read, Err(Error::new(ErrorKind::NotFound, &message)));
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

    let unnamed = files.read(&folder, &file, 1..2);
    files.name(path.clone());
    let read = files.read(&folder, &file, 1..2);
    fs::remove_dir_all(&folder).unwrap();

    let message = format!("No language server named {}.", path.display());
    assert_eq!(unnamed, Err(Error::new(ErrorKind::NotFound, &message)));
    assert_eq!(read, Ok(code(3, &["def print("])));
}

#[test]
fn a_named_file_that_has_gone_from_disk_says_so() {
    let path = temp_path("definitions-code-gone").join("builtins.pyi");
    let files = HitFiles::default();
    files.name(path.clone());

    let read = files.read(
        Path::new(""),
        &HitFile::Outside { path: path.clone() },
        1..2,
    );

    let message = read.unwrap_err().message;
    let start = format!("Tsugiki can't read {}: ", path.display());
    assert!(message.starts_with(&start), "{message}");
}
