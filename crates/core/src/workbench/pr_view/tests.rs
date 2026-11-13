use time::OffsetDateTime;

use super::*;
use crate::github::CommitSha;
use crate::github::issue_content::{Comment, CommentId, HiddenReason, IssueComment, IssueContent};
use crate::github::pull_request_content::{
    Anchor, DiffHunk, Opening, Resolution, Review, ReviewId, Side, Subject, Submission,
    ThreadComment, ThreadCommentId, ThreadId, ThreadLine,
};
use crate::github::pull_requests::{PullRequestFacts, ReviewState};
use crate::login::Login;
use crate::workbench::testing::{
    ViewWorld, backport, bots_backport, fixing, issue, someone, stranger,
};

fn view(world: &ViewWorld, number: u32) -> Option<PrView> {
    PrView::read(&world.reader, &world.settings(), number).unwrap()
}

#[test]
fn a_pr_tsugiki_doesnt_hold_has_no_view() {
    let world = ViewWorld::new("pr-view-none");
    world.has(vec![], vec![issue(1)]);

    assert_eq!(view(&world, 1), None, "an Issue is no PR");
    assert_eq!(view(&world, 5), None);
    world.finish();
}

#[test]
fn a_pr_in_an_issues_tree_shows_its_row_before_tsugiki_has_read_its_conversation() {
    let world = ViewWorld::new("pr-view-before-reading");
    world.has(vec![fixing(5, 1)], vec![issue(1)]);

    let view = view(&world, 5).unwrap();

    assert_eq!(view.row.number, 5);
    assert_eq!(view.row.title, "gh-1: Fix it");
    assert_eq!(view.content, None);
    world.finish();
}

#[test]
fn a_bot_backport_names_the_origin_pr_whose_review_state_it_shares() {
    let world = ViewWorld::new("pr-view-shared-review");
    let manual = backport(3, "[3.14] gh-1: Fix it (GH-1)", vec![Some(someone())]);
    world.has(
        vec![
            stranger(),
            bots_backport(2, "[3.14] gh-1: Fix it (GH-1)"),
            manual,
        ],
        vec![issue(1)],
    );

    assert_eq!(view(&world, 2).unwrap().shares_review_of, Some(1));
    assert_eq!(
        view(&world, 3).unwrap().shares_review_of,
        None,
        "a Manual backport"
    );
    assert_eq!(
        view(&world, 1).unwrap().shares_review_of,
        None,
        "the Origin PR"
    );
    world.finish();
}

/// A PR's Conversation with two Reviews, oldest first, one of them hidden and with two threads.
pub(crate) fn reviewed() -> PullRequestContent {
    // A thread on `line`, whose one comment GitHub numbers `number`.
    let thread = |line: Option<u32>, number, opening| ThreadLine {
        id: ThreadId::new(&format!("T{line:?}")),
        path: "Lib/ast.py".to_owned(),
        opening,
        replies: 1,
        resolution: Resolution::Open,
        anchor: if line.is_none() {
            Anchor::Outdated
        } else {
            Anchor::Current
        },
        subject: Subject::Line {
            side: Side::Right,
            line,
            original: Some(7),
        },
        comments: vec![ThreadComment {
            id: Some(ThreadCommentId::new(number)),
            comment: Comment {
                author: Some(Login::new("some-reviewer")),
                created_at: OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Why?</p>".to_owned(),
                hidden: line.is_none().then_some(HiddenReason::Abuse),
            },
            submission: Submission::Submitted,
        }],
        hunk: DiffHunk::parse("@@ -6,2 +6,2 @@\n x = 1\n-y = 2\n+y = 3"),
    };
    PullRequestContent {
        conversation: IssueContent {
            body_html: "<p>Fixes the crash</p>".to_owned(),
            comments: vec![IssueComment {
                id: Some(CommentId::new(6_065_108_504)),
                comment: Comment {
                    author: None,
                    created_at: OffsetDateTime::UNIX_EPOCH,
                    body_html: "<p>Looks good</p>".to_owned(),
                    hidden: Some(HiddenReason::OffTopic),
                },
            }],
            earlier_comments: 2,
            opened_at: Some(OffsetDateTime::UNIX_EPOCH),
        },
        reviews: vec![
            Review {
                id: Some(ReviewId::new(5_481_796_336)),
                author: Some(Login::new("some-reviewer")),
                state: ReviewState::ChangesRequested,
                commit: CommitSha::parse("0123abc"),
                submitted_at: OffsetDateTime::UNIX_EPOCH,
                body_html: "<p>Some notes</p>".to_owned(),
                hidden: Some(HiddenReason::Spam),
                threads: vec![
                    thread(
                        Some(12),
                        4_224_108_382,
                        Opening::Words {
                            words: "Why not a set?".to_owned(),
                        },
                    ),
                    thread(
                        None,
                        4_240_170_720,
                        Opening::Hidden {
                            reason: HiddenReason::Abuse,
                        },
                    ),
                ],
            },
            // Kept from before Tsugiki asked for GitHub's numbers.
            Review {
                id: None,
                author: None,
                state: ReviewState::Approved,
                commit: None,
                submitted_at: OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1),
                body_html: String::new(),
                hidden: None,
                threads: vec![],
            },
        ],
        earlier_reviews: 3,
        head: CommitSha::parse("0123abc"),
        pending: Vec::new(),
    }
}

#[test]
fn a_pr_shows_its_reviews_oldest_first_with_their_threads_as_last_read() {
    let world = ViewWorld::new("pr-view-after-reading");
    let standalone = PullRequestFacts {
        number: 9,
        ..stranger()
    };
    world.has(vec![standalone], vec![]);
    world
        .store
        .save_pull_request_content(&world.settings().name, 9, reviewed())
        .unwrap();

    let view = view(&world, 9).unwrap();

    assert_eq!(view.row.number, 9);
    assert_eq!(view.content, Some(reviewed()));
    world.finish();
}

#[test]
fn the_window_gets_githubs_number_for_each_comment_and_review() {
    let world = ViewWorld::new("pr-view-numbers");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    world
        .store
        .save_pull_request_content(&world.settings().name, 9, reviewed())
        .unwrap();

    let sent = serde_json::to_value(view(&world, 9).unwrap()).unwrap();

    let content = &sent["content"];
    assert_eq!(
        content["comments"][0]["id"],
        serde_json::json!(6_065_108_504_u64)
    );
    assert_eq!(
        content["reviews"][0]["id"],
        serde_json::json!(5_481_796_336_u64)
    );
    assert_eq!(content["reviews"][1]["id"], serde_json::Value::Null);
    world.finish();
}

#[test]
fn each_read_of_a_pr_replaces_its_reviews() {
    let world = ViewWorld::new("pr-view-read-again");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    let name = world.settings().name;
    world
        .store
        .save_pull_request_content(&name, 9, reviewed())
        .unwrap();
    let mut again = reviewed();
    drop(again.reviews.remove(0));
    again.earlier_reviews = 0;

    world
        .store
        .save_pull_request_content(&name, 9, again.clone())
        .unwrap();

    assert_eq!(view(&world, 9).unwrap().content, Some(again));
    world.finish();
}

#[test]
fn a_pr_read_before_tsugiki_read_reviews_has_none_until_the_next_read() {
    let world = ViewWorld::new("pr-view-no-reviews-yet");
    world.has(
        vec![PullRequestFacts {
            number: 9,
            ..stranger()
        }],
        vec![],
    );
    let conversation = reviewed().conversation;
    world
        .store
        .save_issue_content(&world.settings().name, 9, conversation.clone())
        .unwrap();

    let content = view(&world, 9).unwrap().content.unwrap();

    assert_eq!(content.conversation, conversation);
    assert_eq!((content.reviews, content.earlier_reviews), (vec![], 0));
    world.finish();
}
