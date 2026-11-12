use std::collections::BTreeSet;

use time::OffsetDateTime;

use super::*;
use crate::github::issue_content::IssueComment;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::{LatestReview, PullRequestFacts, ReviewState};
use crate::workbench::WorkbenchRow;
use crate::workbench::testing::{ViewWorld, fixing, issue};

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
    world.has(vec![requested(5), theirs(6), requested(7)], vec![issue(1)]);

    assert_eq!(
        view(&world, 1).unwrap().needs_attention,
        BTreeSet::from([5, 7])
    );

    world.has(vec![theirs(5)], vec![]);
    assert_eq!(
        view(&world, 1).unwrap().needs_attention,
        BTreeSet::from([7]),
        "a PR stops Needing attention once the Review request goes"
    );
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
