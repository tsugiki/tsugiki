//! A PR's Conversation: its description and recent comments, and its Reviews with a line for each thread a
//! Review started (#156). Files shows the threads themselves under their code (#184).

use serde::{Deserialize, Deserializer};
use time::OffsetDateTime;

use super::GitHub;
use super::issue_content::{IssueWire, MinimizedWire, Thread, big_int};
use super::pull_requests::{Person, RateLimit};
use crate::error::Error;
use crate::facts::CommitSha;
use crate::facts::issue_content::Comment;
use crate::facts::pull_request_content::{
    Anchor, DiffHunk, Opening, PullRequestContent, Resolution, Review, ReviewId, Side, Subject,
    Submission, ThreadComment, ThreadCommentId, ThreadId, ThreadLine,
};
use crate::facts::pull_requests::ReviewState;
use crate::repository::RepositoryName;

/// The Reviews and threads the Conversation reads beside the comments. The last 100 Reviews and the last 100
/// threads, with the first 100 comments of each, cost GitHub about 104 requests: one point (ADR 0015). GitHub
/// counts a request for each thread's comments, however many it gives. Each reply in a thread is a Review of
/// its own on GitHub, so a busy PR needs all 100 to hold the Reviews worth showing.
pub(super) const REVIEW_FIELDS: &str = " updatedAt headRefOid reviews(last: 100) { totalCount nodes { id \
                                        fullDatabaseId author { login } state submittedAt bodyHTML \
                                        isMinimized minimizedReason commit { oid } } } \
                                        reviewThreads(last: 100) { nodes { id path line originalLine \
                                        diffSide subjectType isResolved isOutdated comments(first: 100) { \
                                        totalCount nodes { fullDatabaseId pullRequestReview { id } \
                                        author { login } createdAt bodyHTML bodyText isMinimized \
                                        minimizedReason diffHunk } } } }";

/// The longest start of a thread's first comment that Tsugiki keeps. The line cuts it again to fit the
/// pane.
const FIRST_WORDS: usize = 120;

/// GitHub's `isResolved`.
fn resolution<'de, D: Deserializer<'de>>(answer: D) -> Result<Resolution, D::Error> {
    let resolved = bool::deserialize(answer)?;
    Ok(if resolved {
        Resolution::Resolved
    } else {
        Resolution::Open
    })
}

/// GitHub's `isOutdated`.
fn anchor<'de, D: Deserializer<'de>>(answer: D) -> Result<Anchor, D::Error> {
    let outdated = bool::deserialize(answer)?;
    Ok(if outdated {
        Anchor::Outdated
    } else {
        Anchor::Current
    })
}

/// GitHub's node id of a Review, which ties a thread's comments to the Review that wrote them.
#[derive(Deserialize, PartialEq, Eq, Clone)]
struct ReviewNodeId(String);

#[derive(Deserialize)]
struct CommitWire {
    oid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewWire {
    id: ReviewNodeId,
    #[serde(default, deserialize_with = "big_int")]
    full_database_id: Option<u64>,
    author: Option<Person>,
    state: ReviewState,
    #[serde(with = "time::serde::rfc3339::option")]
    submitted_at: Option<OffsetDateTime>,
    #[serde(rename = "bodyHTML")]
    body_html: String,
    #[serde(flatten)]
    minimized: MinimizedWire,
    commit: Option<CommitWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewsWire {
    total_count: u32,
    nodes: Vec<Option<ReviewWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SubjectType {
    Line,
    File,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadCommentWire {
    #[serde(default, deserialize_with = "big_int")]
    full_database_id: Option<u64>,
    pull_request_review: Option<ReviewIdWire>,
    author: Option<Person>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(rename = "bodyHTML")]
    body_html: String,
    body_text: String,
    #[serde(flatten)]
    minimized: MinimizedWire,
    diff_hunk: String,
}

#[derive(Deserialize)]
struct ReviewIdWire {
    id: ReviewNodeId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadCommentsWire {
    total_count: u32,
    nodes: Vec<Option<ThreadCommentWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadWire {
    id: ThreadId,
    path: String,
    line: Option<u32>,
    original_line: Option<u32>,
    diff_side: Side,
    subject_type: SubjectType,
    #[serde(rename = "isResolved", deserialize_with = "resolution")]
    resolution: Resolution,
    #[serde(rename = "isOutdated", deserialize_with = "anchor")]
    anchor: Anchor,
    comments: ThreadCommentsWire,
}

#[derive(Deserialize)]
struct ThreadsWire {
    nodes: Vec<Option<ThreadWire>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequestWire {
    #[serde(flatten)]
    conversation: IssueWire,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
    head_ref_oid: String,
    reviews: ReviewsWire,
    review_threads: ThreadsWire,
}

/// The first line of `text` with words in it, cut to [`FIRST_WORDS`] characters.
fn first_words(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty());
    line.unwrap_or_default().chars().take(FIRST_WORDS).collect()
}

impl ThreadWire {
    /// The id of the Review that started it, and the thread. Nothing, for a thread with no first comment.
    /// A comment of the user's Pending review, `pending`, is a draft, which the replies don't count.
    fn into_line(self, pending: Option<&ReviewNodeId>) -> Option<(ReviewNodeId, ThreadLine)> {
        let total = self.comments.total_count;
        let comments: Vec<ThreadCommentWire> = self.comments.nodes.into_iter().flatten().collect();
        let first = comments.first()?;
        let review = first.pull_request_review.as_ref()?.id.clone();
        let opening = match first.minimized.reason() {
            Some(reason) => Opening::Hidden { reason },
            None => Opening::Words {
                words: first_words(&first.body_text),
            },
        };
        let (subject, hunk) = match self.subject_type {
            SubjectType::Line => (
                Subject::Line {
                    side: self.diff_side,
                    line: self.line,
                    original: self.original_line,
                },
                DiffHunk::parse(&first.diff_hunk),
            ),
            SubjectType::File => (Subject::File, None),
        };
        let comments = comments.into_iter().map(|comment| {
            let drafted = comment
                .pull_request_review
                .is_some_and(|review| Some(&review.id) == pending);
            ThreadComment {
                id: comment.full_database_id.map(ThreadCommentId::new),
                comment: Comment {
                    author: comment.author.map(|person| person.login),
                    created_at: comment.created_at,
                    body_html: comment.body_html,
                    hidden: comment.minimized.reason(),
                },
                submission: if drafted {
                    Submission::Pending
                } else {
                    Submission::Submitted
                },
            }
        });
        let comments: Vec<ThreadComment> = comments.collect();
        let drafts = comments
            .iter()
            .skip(1)
            .filter(|comment| comment.submission == Submission::Pending);
        let drafts = u32::try_from(drafts.count()).unwrap_or(u32::MAX);
        let replies = total.saturating_sub(1).saturating_sub(drafts);
        let thread = ThreadLine {
            id: self.id,
            path: self.path,
            opening,
            replies,
            resolution: self.resolution,
            anchor: self.anchor,
            subject,
            comments,
            hunk,
        };
        Some((review, thread))
    }
}

impl From<PullRequestWire> for PullRequestContent {
    fn from(wire: PullRequestWire) -> Self {
        let read: Vec<ReviewWire> = wire.reviews.nodes.into_iter().flatten().collect();
        // GitHub shows a Pending review to its author alone, so one here is the user's.
        let pending = read
            .iter()
            .find(|review| review.state == ReviewState::Pending)
            .map(|review| review.id.clone());
        let mut threads: Vec<(ReviewNodeId, ThreadLine)> = wire
            .review_threads
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|thread| thread.into_line(pending.as_ref()))
            .collect();
        let drafts = threads.extract_if(.., |(id, _)| Some(&*id) == pending.as_ref());
        let drafts: Vec<ThreadLine> = drafts.map(|(_, thread)| thread).collect();
        let held = u32::try_from(read.len()).unwrap_or(u32::MAX);
        let mut reviews: Vec<Review> = read
            .into_iter()
            .filter_map(|review| {
                let submitted_at = review.submitted_at?;
                let started = threads.extract_if(.., |(id, _)| *id == review.id);
                let started: Vec<ThreadLine> = started.map(|(_, thread)| thread).collect();
                let only_replied = review.state == ReviewState::Commented
                    && review.body_html.trim().is_empty()
                    && started.is_empty();
                (review.state != ReviewState::Pending && !only_replied).then(|| Review {
                    id: review.full_database_id.map(ReviewId::new),
                    author: review.author.map(|person| person.login),
                    state: review.state,
                    commit: review
                        .commit
                        .and_then(|commit| CommitSha::parse(&commit.oid)),
                    submitted_at,
                    body_html: review.body_html,
                    hidden: review.minimized.reason(),
                    threads: started,
                })
            })
            .collect();
        reviews.sort_by_key(|review| review.submitted_at);
        Self {
            conversation: wire.conversation.into(),
            reviews,
            earlier_reviews: wire.reviews.total_count.saturating_sub(held),
            head: CommitSha::parse(&wire.head_ref_oid),
            pending: drafts,
        }
    }
}

/// A PR's Conversation as GitHub gave it, and the points GitHub has left.
#[derive(Debug)]
pub struct PullRequestRead {
    pub content: PullRequestContent,
    /// GitHub's `updatedAt` for the PR at this read, which tells the store whether the head the content
    /// names is newer than the head it keeps.
    pub updated_at: OffsetDateTime,
    pub rate_limit: RateLimit,
}

impl GitHub {
    /// PR `number`'s Conversation: its description, recent comments and Reviews.
    pub fn pull_request_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<PullRequestRead, Error> {
        let (wire, rate_limit): (PullRequestWire, RateLimit) =
            self.conversation(Thread::PullRequest, repository, number)?;
        Ok(PullRequestRead {
            updated_at: wire.updated_at,
            content: wire.into(),
            rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
