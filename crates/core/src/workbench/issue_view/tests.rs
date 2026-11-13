use std::collections::BTreeSet;

use time::OffsetDateTime;

use super::*;
use crate::filters::{Filter, FilterQuery};
use crate::github::issue_content::{Comment, CommentId, IssueComment};
use crate::github::issues::IssueFacts;
use crate::github::labels::Label;
use crate::github::pull_requests::{LatestReview, PullRequestFacts, ReviewState};
use crate::store::FilterResults;
use crate::workbench::testing::{
    ViewWorld, backport, bots_backport, first_issue, fixing, issue, workbench,
};
use crate::workbench::{WorkbenchRow, YourReview};

fn merged(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        state: PrState::Merged,
        ..facts
    }
}

fn closed(facts: PullRequestFacts) -> PullRequestFacts {
    PullRequestFacts {
        state: PrState::Closed,
        ..facts
    }
}

/// Each entry of a tree as its PRs' numbers.
fn numbers(entries: &[TreeEntry]) -> Vec<Vec<u32>> {
    let numbers = |entry: &TreeEntry| entry.rows().iter().map(|row| row.number).collect();
    entries.iter().map(numbers).collect()
}

#[test]
fn origin_prs_that_merged_with_each_backport_done_fold_and_the_view_counts_them() {
    let world = ViewWorld::new("issue-view-fold");
    world.has(
        vec![
            merged(fixing(10, 1)),
            merged(bots_backport(11, "[3.14] gh-1: Fix it (GH-10)")),
            // The bot's Backport conflicted and closed, and a manual one merged in its place.
            closed(bots_backport(12, "[3.13] gh-1: Fix it (GH-10)")),
            merged(backport(13, "[3.13] gh-1: Fix it (GH-10)", vec![])),
            merged(fixing(20, 1)),
            bots_backport(21, "[3.14] gh-1: Fix it (GH-20)"),
            fixing(30, 1),
            merged(backport(40, "[3.13] gh-1: Fix it (GH-99)", vec![])),
            closed(backport(41, "[3.12] gh-1: Fix it (GH-98)", vec![])),
        ],
        vec![issue(1)],
    );

    let shown = view(&world, 1).unwrap();

    assert_eq!(
        numbers(&shown.issue.pull_requests),
        [vec![41], vec![30], vec![20, 21]]
    );
    assert_eq!(numbers(&shown.merged), [vec![40], vec![10, 13, 12, 11]]);
    assert_eq!(shown.merged_count, 5);
    world.finish();
}

/// `IssueView::read` hands `fold` the Matched PRs of the user's Filters.
#[test]
fn a_merged_backport_a_filter_matched_keeps_its_origin_pr_listed() {
    let world = ViewWorld::new("issue-view-fold-matched");
    world.has(
        vec![
            merged(fixing(10, 1)),
            merged(bots_backport(11, "[3.14] gh-1: Fix it (GH-10)")),
            merged(fixing(20, 1)),
        ],
        vec![issue(1)],
    );
    let query: FilterQuery = "is:pr label:needs-backport".to_owned().try_into().unwrap();
    let settings = RepositorySettings {
        filters: vec![Filter {
            name: "Backports".to_owned().try_into().unwrap(),
            query: query.clone(),
        }],
        ..world.settings()
    };
    let found = FilterResults {
        numbers: [11].into(),
        problem: None,
    };
    world
        .store
        .save_filter_results(&settings.name, &query, found)
        .unwrap();

    let shown = IssueView::read(&world.reader, &settings, 1)
        .unwrap()
        .unwrap();

    assert_eq!(numbers(&shown.issue.pull_requests), [vec![10, 11]]);
    assert_eq!(numbers(&shown.merged), [vec![20]]);
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
        assignees: vec![Login::new("some-assignee")],
        ..issue(1)
    };
    world.has(vec![fixing(5, 1)], vec![written]);

    let shown = view(&world, 1).unwrap();

    assert_eq!(shown.issue.number, 1);
    assert_eq!(shown.author, Some(Login::new("issue-author")));
    assert_eq!(shown.assignees, [Login::new("some-assignee")]);
    assert_eq!(pr_row(&shown).number, 5);
    assert_eq!(shown.content, None);
    world.finish();
}

#[test]
fn an_issue_shows_its_labels_as_the_last_sync_read_them() {
    let world = ViewWorld::new("issue-view-labels");
    let labelled = |names: &[&str]| IssueFacts {
        labels: names.iter().map(|name| Label::new(name)).collect(),
        ..issue(1)
    };
    // `Label` ignores case when it compares, so the names are compared as text, to keep GitHub's spelling.
    let shown = |world: &ViewWorld| -> Vec<String> {
        let labels = view(world, 1).unwrap().labels;
        labels
            .iter()
            .map(|label| label.as_str().to_owned())
            .collect()
    };
    world.has(vec![], vec![labelled(&["type-bug", "3.14"])]);
    assert_eq!(shown(&world), ["3.14", "type-bug"]);

    world.has(vec![], vec![labelled(&["type-bug", "3.14", "Easy"])]);
    assert_eq!(shown(&world), ["3.14", "Easy", "type-bug"]);

    world.has(vec![], vec![labelled(&[])]);
    assert_eq!(shown(&world), Vec::<String>::new());
    world.finish();
}

#[test]
fn an_issue_nobody_has_to_act_on_still_has_a_view_with_what_was_last_read() {
    let world = ViewWorld::new("issue-view-after-reading");
    world.has(vec![], vec![issue(1)]);
    let content = IssueContent {
        body_html: "<p>It crashes</p>".to_owned(),
        comments: vec![IssueComment {
            id: Some(CommentId::new(6_065_108_504)),
            comment: Comment {
                author: None,
                created_at: OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Which build?</p>".to_owned(),
                hidden: None,
            },
        }],
        earlier_comments: 0,
        opened_at: Some(OffsetDateTime::UNIX_EPOCH),
    };
    world
        .store
        .save_issue_content(&world.settings().name, 1, content.clone())
        .unwrap();

    let view = view(&world, 1).unwrap();
    assert_eq!(view.content, Some(content));
    // The number a link names, beside what the comment says.
    let sent = serde_json::to_value(&view).unwrap();
    let comment = &sent["content"]["comments"][0];
    assert_eq!(comment["id"], serde_json::json!(6_065_108_504_u64));
    assert_eq!(comment["body_html"], "<p>Which build?</p>");
    world.finish();
}

#[test]
fn an_issue_view_names_the_prs_in_its_tree_that_need_attention() {
    let world = ViewWorld::new("issue-view-attention");
    let theirs = |number| PullRequestFacts {
        author: Some(Login::new("some-author")),
        ..fixing(number, 1)
    };
    let requested = |number| PullRequestFacts {
        review_requests: vec![Login::new("octocat")],
        ..theirs(number)
    };
    let requested_backport = PullRequestFacts {
        review_requests: vec![Login::new("octocat")],
        ..backport(8, "[3.14] gh-1: Fix it (GH-6)", vec![])
    };
    world.has(
        vec![requested(5), theirs(6), requested(7), requested_backport],
        vec![issue(1)],
    );

    assert_eq!(
        view(&world, 1).unwrap().needs_attention,
        BTreeSet::from([5, 7, 8]),
        "a Backport Needs attention by itself, whatever its Origin PR"
    );

    world.has(vec![theirs(5)], vec![]);
    assert_eq!(
        view(&world, 1).unwrap().needs_attention,
        BTreeSet::from([7, 8]),
        "a PR stops Needing attention once the Review request goes"
    );
    world.finish();
}

#[test]
fn a_pr_row_says_whether_the_user_has_reviewed_it() {
    let world = ViewWorld::new("issue-view-review");
    let review = |author: &str, state| LatestReview {
        author: Login::new(author),
        state,
    };
    // A colleague's PR, as the user's comments on their own PR are no Review of it.
    let theirs = |number, issue| PullRequestFacts {
        author: Some(Login::new("some-author")),
        ..fixing(number, issue)
    };
    let reviewed = PullRequestFacts {
        latest_reviews: vec![
            review("some-reviewer", ReviewState::Commented),
            review("octocat", ReviewState::ChangesRequested),
        ],
        ..theirs(5, 1)
    };
    let drafted = PullRequestFacts {
        latest_reviews: vec![review("octocat", ReviewState::Pending)],
        ..theirs(6, 2)
    };
    let others_only = PullRequestFacts {
        latest_reviews: vec![review("some-reviewer", ReviewState::Approved)],
        ..theirs(7, 3)
    };
    let own = PullRequestFacts {
        latest_reviews: vec![review("octocat", ReviewState::Commented)],
        ..fixing(8, 4)
    };
    world.has(
        vec![reviewed, drafted, others_only, own],
        vec![issue(1), issue(2), issue(3), issue(4)],
    );

    let your_review = |number| pr_row(&view(&world, number).unwrap()).your_review.clone();

    assert_eq!(your_review(1), YourReview::Reviewed);
    assert_eq!(
        your_review(2),
        YourReview::Unreviewed,
        "a pending Review is only a draft"
    );
    assert_eq!(your_review(3), YourReview::Unreviewed);
    assert_eq!(
        your_review(4),
        YourReview::Unreviewed,
        "the user's comments on their own PR"
    );
    world.finish();
}
