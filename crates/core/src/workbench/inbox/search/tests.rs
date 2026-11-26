use super::*;

/// Issue 7 titled `title`, by `author`, with `labels`, open or closed.
fn issue_by(title: &str, author: &str, labels: &[&str], state: State) -> SearchRow {
    SearchRow {
        kind: RowKind::Issue,
        number: 7,
        title: title.to_lowercase(),
        author: Some(Login::new(author)),
        labels: labels.iter().map(|label| Label::new(label)).collect(),
        open: state == State::Open,
    }
}

fn issue(title: &str, state: State) -> SearchRow {
    issue_by(title, "some-author", &[], state)
}

/// PR 9 titled `title`, by `some-author`, with no label.
fn pull_request(title: &str, state: State) -> SearchRow {
    SearchRow {
        kind: RowKind::PullRequest,
        number: 9,
        title: title.to_lowercase(),
        author: Some(Login::new("some-author")),
        labels: Vec::new(),
        open: state == State::Open,
    }
}

#[test]
fn bare_words_match_every_word_of_a_title_in_any_case() {
    let search = Search::parse("FIX parser");

    assert!(search.finds(&issue("Fix the parser", State::Open)));
    assert!(!search.finds(&issue("Fix the lexer", State::Open)));
    assert!(
        !search.finds(&issue("Parser", State::Open)),
        "every word must match"
    );
}

#[test]
fn a_hash_number_matches_only_that_number() {
    let search = Search::parse("#9");

    assert!(search.finds(&pull_request("Anything", State::Open)));
    assert!(!search.finds(&issue("Anything", State::Open)));
}

#[test]
fn author_matches_a_row_s_author_in_any_case() {
    let search = Search::parse("author:Some-Author");
    let other = issue_by("It broke", "another-author", &[], State::Open);

    assert!(search.finds(&issue("It broke", State::Open)));
    assert!(!search.finds(&other));
}

#[test]
fn label_matches_a_row_s_label_in_any_case() {
    let search = Search::parse("label:BUG");
    let labelled = issue_by("It broke", "some-author", &["bug"], State::Open);
    let other = issue_by("It broke", "some-author", &["docs"], State::Open);

    assert!(search.finds(&labelled));
    assert!(!search.finds(&other));
}

#[test]
fn is_issue_and_is_pr_match_only_their_kind() {
    assert!(Search::parse("is:issue").finds(&issue("It broke", State::Open)));
    assert!(!Search::parse("is:issue").finds(&pull_request("It broke", State::Open)));
    assert!(Search::parse("is:pr").finds(&pull_request("It broke", State::Open)));
    assert!(!Search::parse("is:pr").finds(&issue("It broke", State::Open)));
    assert!(Search::parse("IS:ISSUE").finds(&issue("It broke", State::Open)));
}

#[test]
fn is_open_matches_open_rows_and_an_unknown_is_value_needs_github() {
    assert!(Search::parse("is:open").finds(&issue("Anything", State::Open)));
    assert!(!Search::parse("is:open").finds(&issue("Anything", State::Closed)));

    let unknown = Search::parse("is:foo");
    assert!(unknown.needs_github());
    assert!(!unknown.finds(&issue("Anything", State::Open)));
}

#[test]
fn the_search_adds_is_open_unless_the_query_names_a_state() {
    assert!(!Search::parse("fix").finds(&issue("Fix it", State::Closed)));
    assert!(Search::parse("fix").finds(&issue("Fix it", State::Open)));

    assert!(Search::parse("is:closed fix").finds(&issue("Fix it", State::Closed)));
    assert!(!Search::parse("is:closed fix").finds(&issue("Fix it", State::Open)));

    // `is:merged` names a state, so Tsugiki adds nothing, and the cache can't answer it either.
    let merged = Search::parse("is:merged fix");
    assert!(merged.needs_github());
    assert!(!merged.finds(&issue("Fix it", State::Open)));
}

#[test]
fn a_qualifier_the_cache_does_not_read_needs_github() {
    for text in [
        "review:approved",
        "state:closed",
        "sort:updated",
        "is:unmerged",
        "author:@me",
        "label:",
        "#abc",
        "-fix",
        "\"two words\"",
    ] {
        let search = Search::parse(text);
        assert!(search.needs_github(), "{text}");
        assert!(!search.finds(&issue("Fix it", State::Open)), "{text}");
    }
}

#[test]
fn an_empty_box_is_no_search_at_all() {
    assert!(Search::parse("  ").is_empty());
    assert!(!Search::parse("  ").needs_github());
    assert!(!Search::parse("fix").is_empty());
}

#[test]
fn an_empty_box_matches_every_row() {
    let search = Search::parse("");

    assert!(search.finds(&issue("Anything", State::Open)));
    assert!(search.finds(&pull_request("Anything", State::Closed)));
    assert!(Search::parse("  ").finds(&issue("Anything", State::Closed)));
}
