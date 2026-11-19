use super::*;
use crate::facts::activity::ActivityFacts;
use crate::facts::issues::{IssueFacts, IssueState};
use crate::facts::patch_ids::ReviewFacts;
use crate::facts::pull_requests::{PrState, PullRequestFacts};
use crate::testing::cpython_settings;
use crate::workbench::testing::{at, fixing, issue, me, someone, stranger};

/// someone's PR `number`, whose title names `title`'s Issue or none.
fn theirs(number: u32, title: &str, state: PrState) -> PullRequestFacts {
    PullRequestFacts {
        number,
        title: title.to_owned(),
        state,
        updated_at: at(i64::from(number)),
        ..stranger()
    }
}

#[test]
fn mute_author_marks_the_trees_of_their_open_items_and_names_each_number_it_touches() {
    let opened = |number, state| IssueFacts {
        author: Some(someone()),
        state: Some(state),
        ..issue(number)
    };
    let pull_requests = vec![
        theirs(10, "gh-1: Fix it", PrState::Open),
        fixing(11, 1),
        theirs(20, "Tidy up", PrState::Open),
        theirs(30, "Tidy more", PrState::Merged),
        theirs(40, "gh-4: Fix it too", PrState::Open),
        theirs(50, "gh-5: Fix it", PrState::Open),
    ];
    let issues = vec![
        opened(1, IssueState::Open),
        issue(4),
        opened(5, IssueState::Closed),
    ];
    let facts = WorkFacts {
        viewer: Some(me()),
        pull_requests,
        issues,
        filters: Default::default(),
        activity: ActivityFacts::default(),
        reviews: ReviewFacts::default(),
    };

    let authored = authored_by(facts, &cpython_settings(), &someone());

    // Issue 1 holds the user's PR 11, and someone's PR 40 sits in Issue 4, which is no item of theirs.
    let marked: Vec<&[u32]> = (authored.marks.iter())
        .map(|mark| mark.numbers.as_slice())
        .collect();
    assert_eq!(marked, [&[20][..], &[1, 10, 11]]);
    assert_eq!(authored.numbers, [1, 5, 10, 11, 20, 30, 40, 50]);
}
