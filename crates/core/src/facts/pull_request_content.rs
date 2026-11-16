//! A PR's Conversation: its description and recent comments, and its Reviews with a line for each thread a
//! Review started (#156). Files shows the threads themselves under their code (#184).

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;

use crate::facts::CommitSha;
use crate::facts::issue_content::{Comment, HiddenReason, IssueContent};
use crate::facts::pull_requests::ReviewState;
use crate::login::Login;

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

/// GitHub's number for a Review, which a link names after `#pullrequestreview-`. The window gets it as a
/// number, as it gets a comment's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
pub struct ReviewId(#[specta(type = specta_typescript::Number)] u64);

impl ReviewId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// GitHub's number for a comment in a review thread, which a link names after `#discussion_r`. The window
/// gets it as a number, as it gets a comment's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
pub struct ThreadCommentId(#[specta(type = specta_typescript::Number)] u64);

impl ThreadCommentId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Someone's Review of a PR, as the Conversation shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Review {
    /// Nothing, for a Review kept from before Tsugiki asked for it, until the next read.
    pub id: Option<ReviewId>,
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
    /// Nothing, for a comment kept from before Tsugiki asked for it, until the next read.
    pub id: Option<ThreadCommentId>,
    #[serde(flatten)]
    pub comment: Comment,
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

/// The start of a thread's line: the first words of its first comment, or the reason that comment is hidden.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Opening {
    Words { words: String },
    Hidden { reason: HiddenReason },
}
