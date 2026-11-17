use super::*;
use crate::facts::issues::IssueFacts;
use crate::facts::pull_requests::PullRequestFacts;
use crate::testing::cpython;
use crate::workbench::testing::{ViewWorld, at, fixing, issue, stranger};

/// An Issue titled `title`, updated at `seconds`.
fn titled_issue(number: u32, title: &str, seconds: i64) -> IssueFacts {
    IssueFacts {
        title: title.to_owned(),
        updated_at: at(seconds),
        ..issue(number)
    }
}

/// A PR titled `title`, in `state`, updated at `seconds`.
fn titled_pr(number: u32, title: &str, state: PrState, seconds: i64) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: title.to_owned(),
        state,
        updated_at: at(seconds),
        ..stranger()
    }
}

/// What quick open finds for each of `typed` in a store that holds `pull_requests` and `issues`, as the
/// number of the item the text names, if it names one the store holds, and the numbers of the others.
fn numbers_found(
    name: &str,
    pull_requests: Vec<PullRequestFacts>,
    issues: Vec<IssueFacts>,
    typed: &[&str],
) -> Vec<(Option<u32>, Vec<u32>)> {
    let world = ViewWorld::new(name);
    world.has(pull_requests, issues);
    let found = typed
        .iter()
        .map(|typed| {
            let found = find(&world.db.reader, &cpython(), typed).unwrap();
            (
                found.named.map(|item| item.number),
                found.others.iter().map(|item| item.number).collect(),
            )
        })
        .collect();
    world.finish();
    found
}

#[test]
fn a_number_names_its_item_apart_in_any_of_its_spellings() {
    let pull_requests = vec![
        titled_pr(100, "gh-12: Fix the crash", PrState::Open, 50),
        titled_pr(101, "Speed up 12 things", PrState::Merged, 60),
        titled_pr(102, "gh-8: Fix it", PrState::Open, 70),
    ];
    let issues = vec![
        titled_issue(12, "It crashes", 1),
        titled_issue(13, "Other", 99),
    ];

    let found = numbers_found(
        "quick-open-number",
        pull_requests,
        issues,
        &["12", "#12", "gh-12", " GH-12 ", "100", "gh-8", "#8"],
    );

    // A title that holds what was typed follows the item it names: the open first, then the newest. A
    // number the cache holds no item of names none, though a title holds it.
    assert_eq!(
        found,
        [
            (Some(12), vec![100, 101]),
            (Some(12), vec![]),
            (Some(12), vec![100]),
            (Some(12), vec![100]),
            (Some(100), vec![]),
            (None, vec![102]),
            (None, vec![]),
        ]
    );
}

#[test]
fn the_item_a_number_names_counts_among_the_listed_count() {
    let pull_requests: Vec<PullRequestFacts> = (21..=50)
        .map(|number| titled_pr(number, "gh-5: Fix it", PrState::Open, i64::from(number)))
        .collect();

    let found = numbers_found(
        "quick-open-number-listed",
        pull_requests,
        vec![titled_issue(5, "It broke", 1)],
        &["gh-5"],
    );

    let newest: Vec<u32> = (32..=50).rev().collect();
    assert_eq!(found, [(Some(5), newest)]);
}

#[test]
fn title_words_find_the_titles_that_hold_each_of_them_in_any_case() {
    let pull_requests = vec![
        titled_pr(1, "Fix the GC crash", PrState::Merged, 90),
        titled_pr(2, "gc: crash in __del__", PrState::Open, 10),
        titled_pr(3, "Fix the parser", PrState::Open, 50),
    ];
    let issues = vec![titled_issue(4, "Ошибка: crash in Ünicode names", 20)];

    let found = numbers_found(
        "quick-open-words",
        pull_requests,
        issues,
        &["crash gc", "CRASH", "ünicode", "parser crash", "#parser"],
    );

    assert_eq!(
        found,
        [
            (None, vec![2, 1]),
            (None, vec![4, 2, 1]),
            (None, vec![4]),
            (None, vec![]),
            (None, vec![])
        ]
    );
}

#[test]
fn nothing_typed_lists_the_open_items_first_and_the_newest_first_at_most_the_listed_count() {
    let pull_requests: Vec<PullRequestFacts> = (1..=30)
        .map(|number| fixing(number, 1000))
        .map(|facts| PullRequestFacts {
            state: if facts.number <= 25 {
                PrState::Merged
            } else {
                PrState::Open
            },
            updated_at: at(i64::from(facts.number)),
            ..facts
        })
        .collect();
    let mut closed = titled_issue(40, "Closed long ago", 100);
    closed.state = Some(IssueState::Closed);
    let unknown = IssueFacts {
        state: None,
        ..titled_issue(41, "Stored before states", 200)
    };

    let found = numbers_found(
        "quick-open-nothing",
        pull_requests,
        vec![closed, unknown],
        &["", "   "],
    );

    let expected: Vec<u32> = [30, 29, 28, 27, 26, 41, 40]
        .into_iter()
        .chain((8..=25).rev())
        .take(LISTED)
        .collect();
    assert_eq!(found, [(None, expected.clone()), (None, expected)]);
}

#[test]
fn it_tells_an_issue_from_a_pr_with_its_state() {
    let world = ViewWorld::new("quick-open-kinds");
    let draft = PullRequestFacts {
        is_draft: true,
        ..titled_pr(7, "gh-8: Fix it", PrState::Open, 5)
    };
    world.has(vec![draft], vec![titled_issue(8, "It broke", 1)]);

    let found = find(&world.db.reader, &cpython(), "8").unwrap();
    let words = find(&world.db.reader, &cpython(), "it").unwrap();
    world.finish();

    assert_eq!(
        found,
        QuickOpenFound {
            named: Some(TitledItem {
                number: 8,
                title: "It broke".to_owned(),
                updated_at: at(1),
                kind: TitledKind::Issue {
                    state: Some(IssueState::Open),
                },
            }),
            others: vec![TitledItem {
                number: 7,
                title: "gh-8: Fix it".to_owned(),
                updated_at: at(5),
                kind: TitledKind::PullRequest {
                    state: PrState::Open,
                    is_draft: true,
                },
            }],
        }
    );
    // The window gets the item the number names apart, and each item's kind beside its number and title,
    // and no time it doesn't show. Words name no item.
    assert_eq!(
        serde_json::to_value(&found).unwrap(),
        serde_json::json!({
            "named": { "kind": "issue", "number": 8, "title": "It broke", "state": "OPEN" },
            "others": [{
                "kind": "pull_request",
                "number": 7,
                "title": "gh-8: Fix it",
                "state": "OPEN",
                "is_draft": true
            }],
        })
    );
    assert_eq!((words.named, words.others.len()), (None, 2));
}

#[test]
fn only_a_whole_number_names_an_item() {
    let parsed = [
        "123",
        "#123",
        "gh-123",
        "123 crash",
        "#",
        "gh-",
        "+5",
        "99999999999",
        "#12a",
    ]
    .map(|typed| Typed::parse(typed).number);

    assert_eq!(
        parsed,
        [
            Some(123),
            Some(123),
            Some(123),
            None,
            None,
            None,
            None,
            None,
            None
        ]
    );
}
