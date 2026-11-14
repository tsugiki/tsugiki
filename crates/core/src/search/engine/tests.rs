use super::*;

fn query(text: &str) -> Query {
    Query {
        text: text.to_owned(),
        case: Case::Ignored,
        words: Words::Within,
        syntax: Syntax::Literal,
    }
}

fn regex(text: &str) -> Query {
    Query {
        syntax: Syntax::Regex,
        ..query(text)
    }
}

/// The text of `range` in `text`, which counts UTF-16 code units.
fn part(text: &str, range: MatchRange) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let (start, end) = (
        usize::try_from(range.start).unwrap(),
        usize::try_from(range.end).unwrap(),
    );
    String::from_utf16(units.get(start..end).unwrap()).unwrap()
}

/// What `lines` holds: each line as its number, its text and the text of each match.
type Found = Vec<(u32, String, Vec<String>)>;

/// The lines of `text` that hold `query`.
fn found(query: &Query, text: &str) -> Found {
    let matcher = Matcher::new(query).unwrap();
    let lines = matcher.lines(&mut searcher(), text.as_bytes()).unwrap();
    lines
        .into_iter()
        .map(|line| {
            let matched = (line.ranges.iter())
                .map(|range| part(&line.text, *range))
                .collect();
            (line.line, line.text, matched)
        })
        .collect()
}

/// The one line of `text` that holds `spam`.
fn spam_line(text: &str) -> LineHit {
    let matcher = Matcher::new(&query("spam")).unwrap();
    let mut lines = matcher.lines(&mut searcher(), text.as_bytes()).unwrap();
    assert_eq!(lines.len(), 1);
    lines.remove(0)
}

#[test]
fn a_literal_matches_whatever_the_case_and_means_no_regex() {
    let text = "a = 1\nprint(A.b)\nab\n";
    let lines = found(&query("a.b"), text);
    assert_eq!(
        lines,
        [(2, "print(A.b)".to_owned(), vec!["A.b".to_owned()])]
    );
}

#[test]
fn match_case_misses_another_case() {
    let matched = Query {
        case: Case::Matched,
        ..query("spam")
    };
    let lines = found(&matched, "Spam\nspam\n");
    assert_eq!(lines, [(2, "spam".to_owned(), vec!["spam".to_owned()])]);
}

#[test]
fn whole_word_misses_the_word_inside_a_longer_one() {
    let whole = Query {
        words: Words::Whole,
        ..query("spam")
    };
    let lines = found(&whole, "spammer\nspam_eggs\nspam(1)\n");
    assert_eq!(lines, [(3, "spam(1)".to_owned(), vec!["spam".to_owned()])]);
}

#[test]
fn a_regex_in_rusts_syntax_finds_each_match_on_a_line() {
    let text = "x = getattr(a) + GetAttr(b)\nnothing\n";
    let lines = found(&regex(r"get\w+\("), text);
    assert_eq!(
        lines,
        [(
            1,
            "x = getattr(a) + GetAttr(b)".to_owned(),
            vec!["getattr(".to_owned(), "GetAttr(".to_owned()]
        )]
    );
}

#[test]
fn a_regex_that_does_not_parse_gives_its_error() {
    let error = Matcher::new(&regex("spam(")).err().unwrap();
    assert!(
        error.message.contains("unclosed group"),
        "{}",
        error.message
    );
}

#[test]
fn an_empty_query_is_refused() {
    let error = Matcher::new(&query("")).err().unwrap();
    assert_eq!(error.message, "There's nothing to search for.");
}

#[test]
fn a_regex_that_matches_nothing_but_the_empty_string_finds_no_line() {
    assert_eq!(found(&regex("x*"), "a\nb\n"), []);
}

#[test]
fn a_binary_file_is_skipped() {
    let matcher = Matcher::new(&query("spam")).unwrap();
    assert!(matcher.lines(&mut searcher(), b"spam\n\0eggs").is_none());
}

#[test]
fn line_endings_stay_out_of_the_text_and_a_regex_end_matches_before_them() {
    let lines = found(&regex("spam$"), "spam\r\neggs spam\r\n");
    assert_eq!(
        lines,
        [
            (1, "spam".to_owned(), vec!["spam".to_owned()]),
            (2, "eggs spam".to_owned(), vec!["spam".to_owned()])
        ]
    );
}

#[test]
fn one_searcher_serves_one_file_after_another() {
    let matcher = Matcher::new(&query("spam")).unwrap();
    let mut searcher = searcher();
    let first = matcher.lines(&mut searcher, b"spam\neggs\n").unwrap();
    let second = matcher.lines(&mut searcher, b"eggs\nspam\n").unwrap();
    assert_eq!((first[0].line, second[0].line), (1, 2));
}

#[test]
fn each_match_on_a_line_counts() {
    let lines = found(&query("spam"), "spam spam spam spam\n");
    assert_eq!(lines[0].2.len(), 4);
}

#[test]
fn ranges_count_utf16_code_units() {
    let hit = spam_line("é😀 spam\n");
    assert_eq!(hit.ranges, [MatchRange { start: 4, end: 8 }]);
}

#[test]
fn text_that_is_not_utf8_is_read_with_replacements() {
    let matcher = Matcher::new(&query("spam")).unwrap();
    let lines = matcher.lines(&mut searcher(), b"\xff spam\n").unwrap();
    assert_eq!(lines[0].text, "\u{fffd} spam");
    assert_eq!(lines[0].ranges, [MatchRange { start: 2, end: 6 }]);
}

#[test]
fn a_long_line_is_cut_around_its_first_match() {
    let hit = spam_line(&format!("{}spam{}", "a".repeat(400), "b".repeat(400)));
    assert_eq!(hit.text.chars().count(), LONGEST_LINE);
    assert_eq!(hit.cut, Cut::Both);
    let lead = u32::try_from(LEAD).unwrap();
    assert_eq!(
        hit.ranges,
        [MatchRange {
            start: lead,
            end: lead + 4
        }]
    );
}

#[test]
fn a_long_line_keeps_its_end_when_the_match_is_near_it() {
    let hit = spam_line(&format!("{}spam", "a".repeat(400)));
    assert_eq!(hit.cut, Cut::Start);
    assert!(hit.text.ends_with("spam"));
    assert_eq!(hit.text.chars().count(), LONGEST_LINE);
}

#[test]
fn a_long_line_with_its_match_near_the_start_is_cut_after_it_only() {
    let hit = spam_line(&format!("spam{}", "a".repeat(400)));
    assert_eq!(hit.cut, Cut::End);
    assert_eq!(hit.ranges, [MatchRange { start: 0, end: 4 }]);
}

#[test]
fn a_cut_drops_the_matches_outside_it_and_clips_one_across_its_edge() {
    let hit = spam_line(&format!("spam{}spam{}", "a".repeat(240), "spam".repeat(10)));
    // Ends 250 characters in, two characters into the third "spam".
    assert_eq!(
        hit.ranges,
        [
            MatchRange { start: 0, end: 4 },
            MatchRange {
                start: 244,
                end: 248
            },
            MatchRange {
                start: 248,
                end: 250
            },
        ]
    );
}

#[test]
fn a_short_line_is_whole() {
    let hit = spam_line("spam");
    assert_eq!(hit.cut, Cut::Neither);
    assert_eq!(hit.text, "spam");
}
