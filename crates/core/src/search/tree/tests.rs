use std::fs;

use super::*;
use crate::github::CommitSha;
use crate::search::testing::Fixture;
use crate::search::{Case, Cut, MatchRange, Query, Syntax, Words};

fn query(text: &str) -> Query {
    Query {
        text: text.to_owned(),
        case: Case::Ignored,
        words: Words::Within,
        syntax: Syntax::Literal,
    }
}

/// Each file a search found, as its path, whether it is generated and the numbers and text of its lines.
type Found = Vec<(String, bool, Vec<(u32, String)>)>;

/// What a search of `fixture` for `query` found, by path, and how it ended.
fn searched(fixture: &Fixture, query: &Query) -> (Found, Outcome) {
    let matcher = Matcher::new(query).unwrap();
    let mut found = Vec::new();
    let outcome = search(
        &fixture.checkout,
        &matcher,
        &AtomicBool::new(false),
        &mut |batch| found.extend(batch),
    )
    .unwrap();
    found.sort_by(|one: &FileHits, other| one.path.cmp(&other.path));
    let found = found
        .into_iter()
        .map(|hits| {
            let lines = (hits.lines.into_iter())
                .map(|line| (line.line, line.text))
                .collect();
            (hits.path, hits.generated, lines)
        })
        .collect();
    (found, outcome)
}

fn pr(name: &str) -> Fixture {
    Fixture::new(
        name,
        &[
            ("Lib/unchanged.py", b"def spam():\n    return 1\n"),
            ("Lib/changed.py", b"old_spam = 1\n"),
            ("Lib/deleted.py", b"spam = 'gone'\n"),
            ("Modules/spam.c", b"int spam(void);\n"),
        ],
        &[
            ("Lib/changed.py", Some(b"new_spam = 2\n".as_slice())),
            ("Lib/added.py", Some(b"import spam\n".as_slice())),
            ("Lib/deleted.py", None),
        ],
    )
}

#[test]
fn finds_matches_in_unchanged_changed_and_added_files_and_none_in_deleted_ones() {
    let fixture = pr("search-tree");
    let (found, outcome) = searched(&fixture, &query("spam"));
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(
        found,
        [
            (
                "Lib/added.py".to_owned(),
                false,
                vec![(1, "import spam".to_owned())]
            ),
            (
                "Lib/changed.py".to_owned(),
                false,
                vec![(1, "new_spam = 2".to_owned())]
            ),
            (
                "Lib/unchanged.py".to_owned(),
                false,
                vec![(1, "def spam():".to_owned())]
            ),
            (
                "Modules/spam.c".to_owned(),
                false,
                vec![(1, "int spam(void);".to_owned())]
            ),
        ]
    );
}

#[test]
fn a_changed_file_is_read_at_the_head_and_not_from_the_base_on_disk() {
    let fixture = pr("search-tree-head");
    let on_disk = fs::read_to_string(fixture.checkout.folder.join("Lib/changed.py")).unwrap();
    assert_eq!(on_disk, "old_spam = 1\n");
    let (found, _) = searched(&fixture, &query("old_spam"));
    assert_eq!(found, []);
}

#[test]
fn an_added_file_is_read_from_the_checkout_where_the_move_wrote_it() {
    let fixture = pr("search-tree-added");
    fs::write(fixture.checkout.folder.join("Lib/added.py"), "on disk\n").unwrap();
    let (found, _) = searched(&fixture, &query("on disk"));
    assert_eq!(
        found,
        [(
            "Lib/added.py".to_owned(),
            false,
            vec![(1, "on disk".to_owned())]
        )]
    );
}

#[test]
fn whole_word_and_match_case_and_a_regex_apply_to_every_file() {
    let fixture = pr("search-tree-options");
    let whole = Query {
        words: Words::Whole,
        ..query("spam")
    };
    let (found, _) = searched(&fixture, &whole);
    let paths: Vec<&str> = found.iter().map(|(path, _, _)| path.as_str()).collect();
    assert_eq!(
        paths,
        ["Lib/added.py", "Lib/unchanged.py", "Modules/spam.c"]
    );
    let regex = Query {
        syntax: Syntax::Regex,
        case: Case::Matched,
        ..query(r"^(new|int) ?\w*")
    };
    let (found, _) = searched(&fixture, &regex);
    let paths: Vec<&str> = found.iter().map(|(path, _, _)| path.as_str()).collect();
    assert_eq!(paths, ["Lib/changed.py", "Modules/spam.c"]);
}

#[test]
fn a_binary_file_is_skipped_and_a_generated_file_says_so() {
    let fixture = Fixture::new(
        "search-tree-kinds",
        &[
            (
                ".gitattributes",
                b"*.gen linguist-generated\nkept.txt linguist-generated=false\n",
            ),
            ("blob.bin", b"spam\0spam"),
            ("clinic.gen", b"spam\n"),
            ("kept.txt", b"spam\n"),
        ],
        &[],
    );
    let (found, _) = searched(&fixture, &query("spam"));
    assert_eq!(
        found,
        [
            ("clinic.gen".to_owned(), true, vec![(1, "spam".to_owned())]),
            ("kept.txt".to_owned(), false, vec![(1, "spam".to_owned())]),
        ]
    );
}

#[test]
fn a_search_stops_at_the_most_matches_and_says_so() {
    // 40 matches to a line, which is short enough to show them all.
    let line = "spam ".repeat(40);
    let text = format!("{line}\n").repeat(75);
    let files: Vec<(String, Vec<u8>)> = (0..2)
        .map(|file| (format!("file{file}.txt"), text.clone().into_bytes()))
        .collect();
    let base: Vec<(&str, &[u8])> = (files.iter())
        .map(|(path, text)| (path.as_str(), text.as_slice()))
        .collect();
    let fixture = Fixture::new("search-tree-most", &base, &[]);
    let matcher = Matcher::new(&query("spam")).unwrap();
    let mut matches = 0;
    let outcome = search(
        &fixture.checkout,
        &matcher,
        &AtomicBool::new(false),
        &mut |batch| {
            let lines = batch.iter().flat_map(|hits| &hits.lines);
            matches += lines.map(LineHit::matches).sum::<u32>();
        },
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Stopped);
    assert_eq!(matches, MOST_MATCHES);
}

#[test]
fn a_file_that_comes_once_the_matches_are_all_found_hands_on_nothing() {
    let matcher = Matcher::new(&query("spam")).unwrap();
    let cancelled = AtomicBool::new(false);
    let run = Run {
        matcher: &matcher,
        cancelled: &cancelled,
        matches: AtomicU32::new(MOST_MATCHES),
        stopped: AtomicBool::new(false),
    };
    let (send, heard) = mpsc::channel();
    run.search(&mut Hand::new(send), "late.txt", b"spam\n");
    assert!(heard.try_recv().is_err());
    assert!(run.over());
}

#[test]
fn a_cancelled_search_reads_no_file() {
    let fixture = pr("search-tree-cancelled");
    let matcher = Matcher::new(&query("spam")).unwrap();
    let mut found = Vec::new();
    let outcome = search(
        &fixture.checkout,
        &matcher,
        &AtomicBool::new(true),
        &mut |batch| found.extend(batch),
    )
    .unwrap();
    assert_eq!((outcome, found), (Outcome::Done, vec![]));
}

#[test]
fn a_file_gone_from_the_checkout_is_left_out() {
    let fixture = pr("search-tree-gone");
    fs::remove_file(fixture.checkout.folder.join("Modules/spam.c")).unwrap();
    let (found, _) = searched(&fixture, &query("int spam"));
    assert_eq!(found, []);
}

#[test]
fn a_link_and_a_submodule_hold_no_file_of_the_tree() {
    let entries = [
        "100644 blob aaa\tLib/a.py",
        "120000 blob bbb\tlink",
        "160000 commit ccc\tsub",
        "100755 blob ddd\trun.sh",
    ];
    let output: Vec<u8> = (entries.iter())
        .flat_map(|entry| entry.bytes().chain([0]))
        .collect();
    let paths: Vec<String> = (parse_tree(&output).into_iter())
        .map(|(path, _)| path)
        .collect();
    assert_eq!(paths, ["Lib/a.py", "run.sh"]);
}

#[test]
fn a_commit_git_does_not_hold_fails_the_search() {
    let mut fixture = pr("search-tree-unknown");
    fixture.checkout.at.head = CommitSha::parse(&"0".repeat(40)).unwrap();
    let matcher = Matcher::new(&query("spam")).unwrap();
    let failed = search(
        &fixture.checkout,
        &matcher,
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert!(failed.is_err());
}

#[test]
fn keep_first_keeps_the_matches_that_fit_and_their_lines() {
    let line = |number, count| LineHit {
        line: number,
        text: String::new(),
        cut: Cut::Neither,
        ranges: vec![MatchRange { start: 0, end: 1 }; count],
    };
    let mut lines = vec![line(1, 2), line(2, 3), line(3, 1)];
    keep_first(&mut lines, 4);
    let kept: Vec<(u32, usize)> = lines
        .iter()
        .map(|line| (line.line, line.ranges.len()))
        .collect();
    assert_eq!(kept, [(1, 2), (2, 2)]);
}

#[test]
fn a_batch_cut_short_ends_the_list_and_a_missing_object_reads_as_nothing() {
    let output = b"abc missing\n111 blob 4\nspam\n222 blob 9\nshort";
    assert_eq!(parse_batch(output), [None, Some(b"spam".to_vec())]);
}
