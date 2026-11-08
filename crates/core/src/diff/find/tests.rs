use super::*;

/// A diff of `a.py`: a header, an `@@` line, then `x = 1` and `Ä x` removed, `X = 2` and `y = 3` added,
/// and `😀 x` as context.
fn diff() -> Diff {
    let patch =
        "diff --git a/a.py b/a.py\n@@ -1,3 +1,4 @@ def x\n-x = 1\n-Ä x\n+X = 2\n+y = 3\n 😀 x\n";
    Diff::parse("3\t2\ta.py\0", patch).unwrap()
}

fn hit(row: u32, start: u32, end: u32) -> Match {
    Match { row, start, end }
}

#[test]
fn a_search_finds_the_text_in_any_case_in_the_code_of_each_row() {
    let found = diff().find("x");

    assert_eq!(
        found.matches,
        [
            hit(2, 0, 1),
            hit(3, 2, 3),
            hit(4, 0, 1),
            // The emoji counts two UTF-16 code units, as the webview counts it.
            hit(6, 3, 4),
        ],
        "the header's `a.py` and the `@@` line's `def x` aren't code"
    );
    assert!(!found.more);
}

#[test]
fn matches_dont_overlap_and_a_letter_with_an_accent_matches_its_capital() {
    let patch = "diff --git a/a.py b/a.py\n@@ -0,0 +1 @@\n+aaaa ää İ\n";
    let diff = Diff::parse("1\t0\ta.py\0", patch).unwrap();

    assert_eq!(diff.find("AA").matches, [hit(2, 0, 2), hit(2, 2, 4)]);
    assert_eq!(diff.find("ÄÄ").matches, [hit(2, 5, 7)]);
    // `İ` is two characters in lower case, so it matches only itself, keeping its length in the row.
    assert_eq!(diff.find("İ").matches, [hit(2, 8, 9)]);
}

#[test]
fn an_empty_query_finds_nothing() {
    assert_eq!(
        diff().find(""),
        Found {
            matches: Vec::new(),
            more: false
        }
    );
}

#[test]
fn a_search_stops_at_the_most_matches_and_says_there_are_more() {
    let line = format!("+{}\n", "a".repeat(100));
    let lines = MOST_MATCHES / 100 + 1;
    let patch = format!(
        "diff --git a/a.py b/a.py\n@@ -0,0 +1,{lines} @@\n{}",
        line.repeat(lines)
    );
    let diff = Diff::parse(&format!("{lines}\t0\ta.py\0"), &patch).unwrap();

    let found = diff.find("a");

    assert_eq!(found.matches.len(), MOST_MATCHES);
    assert!(found.more);
    assert!(!diff.find(&"a".repeat(100)).more);
}
