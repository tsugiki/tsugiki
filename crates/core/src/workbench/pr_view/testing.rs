use time::OffsetDateTime;

use crate::facts::CommitSha;
use crate::facts::issue_content::{Comment, CommentId, HiddenReason, IssueComment, IssueContent};
use crate::facts::pull_request_content::{
    Anchor, DiffHunk, Opening, PullRequestContent, Resolution, Review, ReviewId, Side, Subject,
    Submission, ThreadComment, ThreadCommentId, ThreadId, ThreadLine,
};
use crate::facts::pull_requests::ReviewState;
use crate::login::Login;

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
