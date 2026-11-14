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
    let named = Named::default();
    let at_base = at(&fixture.checkout.at.base);
    let at_head = at(&fixture.checkout.at.head);

    let base = read(&named, &fixture.mirror, &at_base, 1..10);
    let head = read(&named, &fixture.mirror, &at_head, 2..4);

    assert_eq!(base, Ok(code(3, &["one", "two", "three"])));
    assert_eq!(head, Ok(code(4, &["TWO", "three"])));
}

#[test]
fn lines_past_the_end_don_t_come_and_a_read_hands_at_most_two_thousand() {
    let text: String = (1..=2500).map(|n| format!("{n}\n")).collect();

    let past = some_lines(&text, 2499..2600);
    let many = some_lines(&text, 0..u32::MAX);

    assert_eq!(past, code(2500, &["2499", "2500"]));
    assert_eq!(many.count, 2500);
    assert_eq!(many.lines.len(), 2000);
    assert_eq!(many.lines.first().map(String::as_str), Some("1"));
}

#[test]
fn a_file_the_commit_doesn_t_hold_says_so() {
    let fixture = fixture("definitions-code-missing");
    let commit = fixture.checkout.at.base.clone();
    let missing = HitFile::Repository {
        path: "b.py".to_owned(),
        commit: commit.clone(),
    };

    let read = read(&Named::default(), &fixture.mirror, &missing, 1..10);

    let short = commit.as_str().get(..7).unwrap();
    let message = format!("Tsugiki's copy of the Repository has no b.py at {short}.");
    assert_eq!(read, Err(Error::new(ErrorKind::NotFound, &message)));
}

#[test]
fn a_file_outside_the_repository_is_read_from_disk_once_a_server_named_it() {
    let folder = temp_path("definitions-code-outside");
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join("builtins.pyi");
    fs::write(&path, "def print(\n    *values: object,\n): ...\n").unwrap();
    let named = Named::default();
    let file = HitFile::Outside { path: path.clone() };

    let unnamed = read(&named, &folder, &file, 1..2);
    named.add(path.clone());
    let read = read(&named, &folder, &file, 1..2);
    fs::remove_dir_all(&folder).unwrap();

    let message = format!("No language server named {}.", path.display());
    assert_eq!(unnamed, Err(Error::new(ErrorKind::NotFound, &message)));
    assert_eq!(read, Ok(code(3, &["def print("])));
}
