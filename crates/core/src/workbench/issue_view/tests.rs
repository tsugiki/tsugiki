use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::IssueComment;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{LatestReview, PullRequestFacts, ReviewState};
use crate::workbench::WorkbenchRow;
use crate::workbench::testing::{
    ViewWorld, backport, bots_backport, first_issue, fixing, issue, workbench,
};

fn merged(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        state: PrState::Merged,
        ..facts
    }
}

/// Each entry of a tree as its PRs' numbers.
fn numbers(entries: &[TreeEntry]) -> Vec<Vec<u32>> {
    let numbers = |entry: &TreeEntry| entry.rows().iter().map(|row| row.number).collect();
    entries.iter().map(numbers).collect()
}

#[test]
fn origin_prs_that_merged_with_each_backport_fold_and_the_view_counts_them() {
    let world = ViewWorld::new("issue-view-fold");
    world.has(
        vec![
            merged(fixing(10, 1)),
            merged(bots_backport(11, "[3.14] gh-1: Fix it (GH-10)")),
            merged(fixing(20, 1)),
            bots_backport(21, "[3.14] gh-1: Fix it (GH-20)"),
            fixing(30, 1),
            merged(backport(40, "[3.13] gh-1: Fix it (GH-99)", vec![])),
        ],
        vec![issue(1)],
    );

    let shown = view(&world, 1).unwrap();

    assert_eq!(
        numbers(&shown.issue.pull_requests),
        [vec![30], vec![20, 21]]
    );
    assert_eq!(numbers(&shown.merged), [vec![40], vec![10, 11]]);
    assert_eq!(shown.merged_count, 3);
    world.finish();
}

#[test]
fn an_issue_whose_prs_all_merged_lists_none_beside_the_fold() {
    let world = ViewWorld::new("issue-view-all-merged");
    world.has(
        vec![merged(fixing(10, 1)), merged(fixing(11, 1))],
        vec![issue(1)],
    );

    let shown = view(&world, 1).unwrap();

    assert_eq!(numbers(&shown.issue.pull_requests), Vec::<Vec<u32>>::new());
    assert_eq!(shown.merged_count, 2);
    world.finish();
}

#[test]
fn a_merged_pr_that_needs_attention_or_matched_a_filter_keeps_its_origin_pr_out_of_the_fold() {
    // The open PR 30 keeps the Issue on the Workbench, where the test reads its tree.
    let found = workbench(
        vec![
            merged(fixing(10, 1)),
            merged(bots_backport(11, "[3.14] gh-1: Fix it (GH-10)")),
            merged(fixing(20, 1)),
            fixing(30, 1),
        ],
        vec![issue(1)],
    );
    let entries = first_issue(&found).pull_requests.clone();
    let without = |mut entries: Vec<Vec<u32>>| {
        entries.retain(|entry| entry != &[30]);
        entries
    };

    let (open, folded) = fold(entries.clone(), |_| false);
    assert_eq!(without(numbers(&open)), Vec::<Vec<u32>>::new());
    assert_eq!(numbers(&folded), [vec![10, 11], vec![20]]);

    let (open, folded) = fold(entries, |number| number == 11);
    assert_eq!(without(numbers(&open)), [vec![10, 11]]);
    assert_eq!(numbers(&folded), [vec![20]]);
}

fn view(world: &ViewWorld, number: u32) -> Option<IssueView> {
    IssueView::read(&world.reader, &world.settings(), number).unwrap()
}

fn pr_row(view: &IssueView) -> &WorkbenchRow {
    view.issue.pull_requests[0].rows()[0]
}

#[test]
fn an_issue_tsugiki_doesnt_hold_has_no_view() {
    let world = ViewWorld::new("issue-view-none");
    assert_eq!(view(&world, 1), None);
    world.finish();
}

#[test]
fn an_issue_shows_its_author_and_prs_before_tsugiki_has_read_what_it_says() {
    let world = ViewWorld::new("issue-view-before-reading");
    let written = IssueFacts {
        author: Some(Login::new("issue-author")),
        ..issue(1)
    };
    world.has(vec![fixing(5, 1)], vec![written]);

    let shown = view(&world, 1).unwrap();

    assert_eq!(shown.issue.number, 1);
    assert_eq!(shown.author, Some(Login::new("issue-author")));
    assert_eq!(pr_row(&shown).number, 5);
    assert_eq!(shown.content, None);
    world.finish();
}

#[test]
fn an_issue_nobody_has_to_act_on_still_has_a_view_with_what_was_last_read() {
    let world = ViewWorld::new("issue-view-after-reading");
    world.has(vec![], vec![issue(1)]);
    let content = IssueContent {
        body_html: "<p>It crashes</p>".to_owned(),
        comments: vec![IssueComment {
            author: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            body_html: "<p>Which build?</p>".to_owned(),
            hidden: None,
        }],
        earlier_comments: 0,
    };
    world
        .store
        .save_issue_content(&world.settings().name, 1, content.clone())
        .unwrap();

    assert_eq!(view(&world, 1).unwrap().content, Some(content));
    world.finish();
}

#[test]
fn a_pr_row_says_how_the_user_last_reviewed_it() {
    let world = ViewWorld::new("issue-view-review");
    let review = |author: &str, state| LatestReview {
        author: Login::new(author),
        state,
    };
    let reviewed = PullRequestFacts {
        latest_reviews: vec![
            review("some-reviewer", ReviewState::Commented),
            review("octocat", ReviewState::ChangesRequested),
        ],
        ..fixing(5, 1)
    };
    let drafted = PullRequestFacts {
        latest_reviews: vec![review("octocat", ReviewState::Pending)],
        ..fixing(6, 2)
    };
    let others_only = PullRequestFacts {
        latest_reviews: vec![review("some-reviewer", ReviewState::Approved)],
        ..fixing(7, 3)
    };
    world.has(
        vec![reviewed, drafted, others_only],
        vec![issue(1), issue(2), issue(3)],
    );

    let your_review = |number| pr_row(&view(&world, number).unwrap()).your_review;

    assert_eq!(your_review(1), Some(ReviewState::ChangesRequested));
    assert_eq!(your_review(2), None, "a pending Review is only a draft");
    assert_eq!(your_review(3), None);
    world.finish();
}
