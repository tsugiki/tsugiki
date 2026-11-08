//! A PR's Conversation: its description and recent comments, and its Reviews with a line for each thread a
//! Review started (#156). Files shows the threads themselves under their code (#184).

use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use time::OffsetDateTime;

use super::issue_content::{
    HiddenReason, IssueComment, IssueContent, IssueWire, MinimizedWire, Thread,
};
use super::pull_requests::{Person, RateLimit, ReviewState};
use super::{CommitSha, GitHub, Login};
use crate::error::Error;
use crate::repository::RepositoryName;

/// The Reviews and threads the Conversation reads beside the comments. The last 100 Reviews and the last 100
/// threads, with the first 100 comments of each, cost GitHub about 104 requests: one point (ADR 0015). GitHub
/// counts a request for each thread's comments, however many it gives. Each reply in a thread is a Review of
/// its own on GitHub, so a busy PR needs all 100 to hold the Reviews worth showing.
pub(super) const REVIEW_FIELDS: &str = " headRefOid reviews(last: 100) { totalCount nodes { id author { login } state \
                                        submittedAt bodyHTML isMinimized minimizedReason commit { oid } } } \
                                        reviewThreads(last: 100) { nodes { id path line originalLine \
                                        diffSide subjectType isResolved isOutdated comments(first: 100) { \
                                        totalCount nodes { pullRequestReview { id } author { login } \
                                        createdAt bodyHTML bodyText isMinimized minimizedReason diffHunk } } } }";

/// The longest start of a thread's first comment that Tsugiki keeps. The line cuts it again to fit the
/// pane.
const FIRST_WORDS: usize = 120;

/// A PR's description, its recent comments, and its Reviews.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct PullRequestContent {
    #[serde(flatten)]
    pub conversation: IssueContent,
    /// Its Reviews, oldest first. A Review that only replied in threads others started isn't one here, as
    /// GitHub doesn't show it either. Nor is the user's own Review before they submit it.
    pub reviews: Vec<Review>,
    /// The count of older Reviews that only GitHub holds. Some may only have replied in threads, which GitHub
    /// doesn't show as Reviews, so the view says there are more without the count.
    pub earlier_reviews: u32,
    /// The PR's head as GitHub had it at this read, which the threads' lines count in. Files places them only
    /// on a diff that ends there.
    #[serde(skip)]
    #[specta(skip)]
    pub head: Option<CommitSha>,
    /// The threads the user's Pending review started, which only they see until they submit it on
    /// github.com. Files shows them, and the Conversation leaves them out until Tsugiki can submit them.
    #[serde(skip)]
    #[specta(skip)]
    pub pending: Vec<ThreadLine>,
}

impl PullRequestContent {
    /// Every thread: those of each Review, then those of the user's Pending review.
    pub fn threads(&self) -> impl Iterator<Item = &ThreadLine> {
        let reviews = self.reviews.iter().flat_map(|review| &review.threads);
        reviews.chain(&self.pending)
    }
}

/// GitHub's id of a review thread, which a thread's line in the Conversation names to open it in Files.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize, Type)]
#[serde(transparent)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A thread's subject: a whole file, or a line on one side of the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    /// The whole file.
    File,
    /// Lines of the file at the head, on the right, or at the merge base, on the left.
    Line {
        side: Side,
        /// The line it is on now. Nothing, when the code under it has changed and GitHub can't place it in
        /// the diff any more.
        line: Option<u32>,
        /// The line it was on when it began.
        original: Option<u32>,
    },
}

/// The version of the file a thread's line is in, as GitHub's `diffSide` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Side {
    /// The merge base's.
    Left,
    /// The head's.
    Right,
}

/// Someone's Review of a PR, as the Conversation shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Review {
    /// Nothing, for a Review whose author's account has gone.
    pub author: Option<Login>,
    pub state: ReviewState,
    /// The commit it reviewed. Nothing, when GitHub no longer has it.
    #[specta(type = Option<String>)]
    pub commit: Option<CommitSha>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub submitted_at: OffsetDateTime,
    /// GitHub's rendering, written by a stranger, and empty for a Review that says nothing beyond its state.
    /// The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
    /// The reason its body is hidden on GitHub. Nothing, for one that isn't.
    pub hidden: Option<HiddenReason>,
    /// A line for each thread it started, in the order GitHub gives them.
    pub threads: Vec<ThreadLine>,
}

/// One thread a Review started: a line of the Conversation, and the comments Files shows under the code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ThreadLine {
    /// The thread, which a click on the line opens in Files.
    pub id: ThreadId,
    /// The file, as GitHub names it.
    pub path: String,
    /// The whole file, or the line it is on and the line it began on.
    pub subject: Subject,
    pub opening: Opening,
    /// The comments after the first.
    pub replies: u32,
    pub resolution: Resolution,
    pub anchor: Anchor,
    /// Its first 100 comments, oldest first, which Files shows under the code. The window reads them with
    /// the diff, not with the Conversation.
    #[serde(skip)]
    #[specta(skip)]
    pub comments: Vec<ThreadComment>,
    /// The code it was written against, which Files shows with an Outdated thread. Nothing, for a thread
    /// on a whole file, whose hunk GitHub fills with a placeholder.
    #[serde(skip)]
    #[specta(skip)]
    pub hunk: Option<DiffHunk>,
}

/// A comment in a review thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ThreadComment {
    #[serde(flatten)]
    pub comment: IssueComment,
    pub submission: Submission,
}

/// Whether others can read a comment, or it is a draft in the user's Pending review, which only they see
/// until they submit it on github.com.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Submission {
    Submitted,
    Pending,
}

/// The hunk GitHub stores with a thread's first comment: its `@@` line, then the lines of the diff down to
/// the line the thread began on. It keeps the text it was read from, to store, and its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    text: String,
    rows: Vec<CodeRow>,
}

/// A row of a stored hunk: its `@@` line, a line of the diff, or what git says in place of a line, such as
/// a missing newline at the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CodeRow {
    Hunk { text: String },
    Context { old: u32, new: u32, text: String },
    Added { new: u32, text: String },
    Removed { old: u32, text: String },
    Note { text: String },
}

impl DiffHunk {
    /// The hunk `text` holds. Nothing, if its `@@` line doesn't say where it starts.
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let header = lines.next()?;
        let (mut old, mut new) = starts(header)?;
        let mut rows = vec![CodeRow::Hunk {
            text: header.to_owned(),
        }];
        for line in lines {
            let (marker, code) = line.split_at_checked(1).unwrap_or((" ", line));
            let code = code.to_owned();
            rows.push(match marker {
                "+" => {
                    new = new.saturating_add(1);
                    CodeRow::Added {
                        new: new - 1,
                        text: code,
                    }
                }
                "-" => {
                    old = old.saturating_add(1);
                    CodeRow::Removed {
                        old: old - 1,
                        text: code,
                    }
                }
                "\\" => CodeRow::Note {
                    text: line.to_owned(),
                },
                _ => {
                    old = old.saturating_add(1);
                    new = new.saturating_add(1);
                    CodeRow::Context {
                        old: old - 1,
                        new: new - 1,
                        text: code,
                    }
                }
            });
        }
        Some(Self {
            text: text.to_owned(),
            rows,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Its rows, the `@@` line first.
    pub fn rows(&self) -> &[CodeRow] {
        &self.rows
    }
}

/// The first lines at the merge base and at the head that a `@@ -a,b +c,d @@` line names.
fn starts(header: &str) -> Option<(u32, u32)> {
    let ranges = header.strip_prefix("@@ -")?;
    let (old, rest) = ranges.split_once(" +")?;
    let (new, _) = rest.split_once(' ')?;
    let first = |range: &str| range.split(',').next()?.parse::<u32>().ok();
    Some((first(old)?, first(new)?))
}

impl ThreadLine {
    /// The count of later comments that only GitHub holds.
    pub fn later_comments(&self) -> u32 {
        let held = u32::try_from(self.comments.len()).unwrap_or(u32::MAX);
        (self.replies + 1).saturating_sub(held)
    }
}

/// Whether someone marked a thread resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Open,
    Resolved,
}

/// Whether the code under a thread has changed since it began.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Anchor {
    Current,
    Outdated,
}

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

/// The start of a thread's line: the first words of its first comment, or the reason that comment is hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Opening {
    Words { words: String },
    Hidden { reason: HiddenReason },
}

#[derive(Deserialize, PartialEq, Eq, Clone)]
struct ReviewId(String);

#[derive(Deserialize)]
struct CommitWire {
    oid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewWire {
    id: ReviewId,
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
    id: ReviewId,
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
    fn into_line(self, pending: Option<&ReviewId>) -> Option<(ReviewId, ThreadLine)> {
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
                comment: IssueComment {
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
        let mut threads: Vec<(ReviewId, ThreadLine)> = wire
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
            content: wire.into(),
            rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
