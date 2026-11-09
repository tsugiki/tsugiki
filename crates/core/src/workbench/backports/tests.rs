use crate::github::CommitSha;
use crate::github::pull_requests::OffDefault;
use crate::github::pull_requests::{PrState, PullRequestFacts};
use crate::links::BackportKind;
use crate::workbench::testing::{
    at, backport, bots_backport, first_issue, fixing, issue, me, mine, outline, own, stranger,
    workbench,
};
use crate::workbench::{TreeEntry, Workbench};

/// The first Issue's lines, as each PR's number with the numbers beneath it.
fn tree_of(workbench: &Workbench) -> Vec<(u32, Vec<u32>)> {
    first_issue(workbench)
        .pull_requests
        .iter()
        .map(|entry| match entry {
            TreeEntry::Pr(tree) => (
                tree.row.number,
                tree.backports
                    .iter()
                    .map(|below| below.row.number)
                    .collect(),
            ),
            TreeEntry::Backport(backport) => (backport.row.number, vec![]),
        })
        .collect()
}

#[test]
fn the_window_gets_a_backports_kind_beside_its_row() {
    let found = workbench(
        vec![
            fixing(4, 1),
            backport(5, "[3.14] gh-1: Fix it (GH-4)", vec![]),
            backport(6, "[3.13] gh-1: Fix it (GH-99)", vec![]),
        ],
        vec![issue(1)],
    );
    let sent = serde_json::to_value(&found).unwrap();
    let entries = &sent["quiet"][0]["pull_requests"];
    assert_eq!(entries[0]["backports"][0]["number"], 5);
    assert_eq!(entries[0]["backports"][0]["backport"], "manual");
    assert_eq!(entries[1]["kind"], "backport");
    assert_eq!(entries[1]["number"], 6);
    assert_eq!(entries[1]["backport"], "manual");
}

#[test]
fn a_backport_sits_under_its_origin_pr_with_its_kind() {
    let found = workbench(
        vec![
            fixing(10, 1),
            bots_backport(11, "[3.14] gh-1: Fix it (GH-10)"),
            backport(12, "[3.13] gh-1: Fix it (GH-10)", vec![]),
            bots_backport(13, "[3.12] gh-1: Fix it (GH-99)"),
        ],
        vec![issue(1)],
    );
    assert_eq!(tree_of(&found), [(10, vec![11, 12]), (13, vec![])]);
    let kinds: Vec<_> = first_issue(&found)
        .pull_requests
        .iter()
        .flat_map(|entry| match entry {
            TreeEntry::Pr(tree) => tree.backports.clone(),
            TreeEntry::Backport(backport) => vec![backport.clone()],
        })
        .map(|row| row.backport)
        .collect();
    assert_eq!(
        kinds,
        [BackportKind::Bot, BackportKind::Manual, BackportKind::Bot]
    );
}

#[test]
fn a_backport_under_a_cherry_pick_line_finds_its_origin_pr() {
    let sha = CommitSha::parse("0123abc").unwrap();
    let origin = PullRequestFacts {
        state: PrState::Merged,
        merge_commit: Some(sha.clone()),
        base: None,
        head: None,
        ..fixing(10, 1)
    };
    let by_hand = PullRequestFacts {
        off_default: Some(OffDefault {
            description: String::new(),
            cherry_picked_from: vec![sha],
            committers: vec![],
        }),
        ..own(backport(11, "gh-1: Fix it on 3.14", vec![]))
    };
    let found = workbench(vec![origin, by_hand], vec![issue(1)]);
    assert_eq!(tree_of(&found), [(10, vec![11])]);
}

#[test]
fn a_backport_of_another_issues_pr_brings_that_pr_under_its_own_issue() {
    let origin = fixing(10, 1);
    let found = workbench(
        vec![origin, backport(11, "[3.14] gh-2: Fix it (GH-10)", vec![])],
        vec![issue(1), issue(2)],
    );
    assert_eq!(
        outline(&found),
        [("issue", 2, vec![10, 11]), ("issue", 1, vec![10])]
    );
}

#[test]
fn a_backport_that_fixes_no_issue_follows_its_origin_pr() {
    let both = PullRequestFacts {
        closing_issues: vec![2],
        ..fixing(10, 1)
    };
    let found = workbench(
        vec![both, backport(11, "[3.14] Fix it (GH-10)", vec![])],
        vec![issue(1), issue(2)],
    );
    assert_eq!(
        outline(&found),
        [("issue", 2, vec![10, 11]), ("issue", 1, vec![10, 11])]
    );
}

#[test]
fn a_backport_of_a_backport_stands_by_itself_so_a_tree_stays_two_levels() {
    let found = workbench(
        vec![
            fixing(10, 1),
            backport(11, "[3.14] gh-1: Fix it (GH-10)", vec![]),
            backport(12, "[3.13] gh-1: Fix it (GH-11)", vec![]),
        ],
        vec![issue(1)],
    );
    assert_eq!(tree_of(&found), [(10, vec![11]), (12, vec![])]);
}

#[test]
fn a_users_backport_lists_an_origin_pr_that_is_not_the_users() {
    let found = workbench(
        vec![
            PullRequestFacts {
                number: 10,
                title: "Fix it".to_owned(),
                ..stranger()
            },
            own(backport(11, "[3.14] Fix it (GH-10)", vec![])),
        ],
        vec![],
    );
    assert_eq!(outline(&found), [("standalone", 10, vec![11])]);
}

#[test]
fn a_backport_standing_by_itself_takes_its_place_by_its_time() {
    let later = PullRequestFacts {
        updated_at: at(9),
        ..own(backport(11, "[3.14] Fix it (GH-10)", vec![]))
    };
    let found = workbench(vec![mine(20), later], vec![]);
    assert_eq!(
        outline(&found),
        [
            ("standalone backport", 11, vec![]),
            ("standalone", 20, vec![])
        ]
    );
}

#[test]
fn someone_elses_backport_with_no_origin_pr_and_no_issue_isnt_listed() {
    let found = workbench(vec![backport(11, "[3.14] Fix it (GH-10)", vec![])], vec![]);
    assert_eq!(outline(&found), []);
}

#[test]
fn a_review_request_on_a_backport_lists_its_issue() {
    let requested = PullRequestFacts {
        review_requests: vec![me()],
        ..backport(11, "[3.14] gh-1: Fix it (GH-99)", vec![])
    };
    let found = workbench(vec![requested], vec![issue(1)]);
    assert_eq!(outline(&found), [("issue", 1, vec![11])]);
}

#[test]
fn a_tree_holds_merged_and_closed_prs_and_says_which_they_are() {
    let in_state = |number, state| PullRequestFacts {
        number,
        state,
        title: "gh-1: Fix it".to_owned(),
        ..stranger()
    };
    let found = workbench(
        vec![
            fixing(10, 1),
            in_state(11, PrState::Merged),
            in_state(12, PrState::Closed),
        ],
        vec![issue(1)],
    );
    let states: Vec<_> = first_issue(&found)
        .pull_requests
        .iter()
        .flat_map(TreeEntry::rows)
        .map(|row| row.state)
        .collect();
    assert_eq!(states, [PrState::Open, PrState::Merged, PrState::Closed]);
}
