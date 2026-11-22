//! An Issue's description and recent comments, as the Conversation shows them.

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::facts::pull_requests::NodeId;
use crate::login::Login;

/// The reason someone gave for hiding a comment on GitHub. The comment folds to it until the user opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum HiddenReason {
    Abuse,
    OffTopic,
    Outdated,
    Resolved,
    Duplicate,
    Spam,
    /// GitHub gave a reason Tsugiki doesn't know, or none.
    Unstated,
}

/// GitHub's number for a comment on an Issue or in a PR's Conversation, which a link names after
/// `#issuecomment-`. The window gets it as a number, which holds it exactly up to 2^53.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
pub struct CommentId(#[specta(type = specta_typescript::Number)] u64);

impl CommentId {
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A comment on an Issue, or in a PR's Conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueComment {
    /// Nothing, for a comment kept from before Tsugiki asked for it, until the next read.
    pub id: Option<CommentId>,
    #[serde(flatten)]
    pub comment: Comment,
}

/// What a comment says and who wrote it when, on an Issue, in a PR's Conversation or in a review thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Comment {
    /// Nothing, for a comment whose author's account has gone.
    pub author: Option<Login>,
    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    pub created_at: OffsetDateTime,
    /// GitHub's rendering, written by a stranger. The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
    /// The reason it is hidden on GitHub. Nothing, for a comment that isn't.
    pub hidden: Option<HiddenReason>,
}

/// An Issue's description and its recent comments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueContent {
    /// GitHub's ID of the Issue or PR, which a write names it by (#914). Nothing, for a read kept from
    /// before Tsugiki asked, until the next read. The window has no use for it.
    #[serde(skip)]
    #[specta(skip)]
    pub id: Option<NodeId>,
    /// GitHub's rendering, written by a stranger. The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
    /// The most recent comments, oldest first.
    pub comments: Vec<IssueComment>,
    /// The count of older comments that only GitHub shows.
    pub earlier_comments: u32,
    /// The time its author opened it. Nothing, for a read kept from before Tsugiki asked, until the next
    /// read.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub opened_at: Option<OffsetDateTime>,
}
