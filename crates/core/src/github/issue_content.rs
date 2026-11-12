//! The description and recent comments of one Issue or PR, as GitHub renders them. Only the Issue view and
//! the PR view's Conversation show them, so Tsugiki asks when one opens and while it's on screen. A refresh
//! reads ahead only the Conversations of the PRs the user's work reaches (ADR 0015).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::pull_requests::{Person, RateLimit};
use super::{GitHub, Request};
use crate::error::{Error, ErrorKind};
use crate::login::Login;
use crate::repository::RepositoryName;

/// Whose conversation a query reads. GitHub gives an Issue and a PR the same description and comments, and
/// a PR its Reviews as well.
#[derive(Debug, Clone, Copy)]
pub(super) enum Thread {
    Issue,
    PullRequest,
}

impl Thread {
    const fn operation(self) -> &'static str {
        match self {
            Self::Issue => "FetchIssueContent",
            Self::PullRequest => "FetchPullRequestContent",
        }
    }

    /// The field of `repository` that holds it.
    const fn field(self) -> &'static str {
        match self {
            Self::Issue => "issue",
            Self::PullRequest => "pullRequest",
        }
    }

    const fn noun(self) -> &'static str {
        match self {
            Self::Issue => "Issue",
            Self::PullRequest => "PR",
        }
    }

    const fn more_fields(self) -> &'static str {
        match self {
            Self::Issue => "",
            Self::PullRequest => super::pull_request_content::REVIEW_FIELDS,
        }
    }

    pub(super) fn query(self) -> String {
        format!(
            "query {}($owner: String!, $name: String!, $number: Int!) {{ \
             repository(owner: $owner, name: $name) {{ {}(number: $number) {{ {CONVERSATION_FIELDS}{} }} }} \
             rateLimit {{ remaining resetAt }} }}",
            self.operation(),
            self.field(),
            self.more_fields(),
        )
    }
}

/// The fields an Issue and a PR share: the description, when it was opened, and the last 30 comments.
pub(super) const CONVERSATION_FIELDS: &str = "bodyHTML createdAt comments(last: 30) { totalCount nodes { \
                                              author { login } createdAt bodyHTML isMinimized minimizedReason } }";

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

/// GitHub's word on whether a comment, a Review or a thread's first comment is hidden, and the reason.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct MinimizedWire {
    is_minimized: bool,
    minimized_reason: Option<String>,
}

impl MinimizedWire {
    /// The reason GitHub gives in `minimizedReason`, which it spells in lower case, as "off-topic". Nothing
    /// for a comment that isn't hidden.
    pub(super) fn reason(&self) -> Option<HiddenReason> {
        if !self.is_minimized {
            return None;
        }
        let reason = self.minimized_reason.as_deref().unwrap_or_default();
        Some(
            match reason.to_ascii_lowercase().replace('_', "-").as_str() {
                "abuse" => HiddenReason::Abuse,
                "off-topic" => HiddenReason::OffTopic,
                "outdated" => HiddenReason::Outdated,
                "resolved" => HiddenReason::Resolved,
                "duplicate" => HiddenReason::Duplicate,
                "spam" => HiddenReason::Spam,
                _ => HiddenReason::Unstated,
            },
        )
    }
}

/// A comment on an Issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct IssueComment {
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
    /// GitHub's rendering, written by a stranger. The webview's content security policy holds it (ADR 0005).
    pub body_html: String,
    /// The most recent comments, oldest first.
    pub comments: Vec<IssueComment>,
    /// The count of older comments that only GitHub shows.
    pub earlier_comments: u32,
    /// When its author opened it. Nothing, for a read kept from before Tsugiki asked, until the next read.
    #[serde(with = "time::serde::rfc3339::option")]
    #[specta(type = Option<String>)]
    pub opened_at: Option<OffsetDateTime>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentWire {
    author: Option<Person>,
    #[serde(with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    #[serde(rename = "bodyHTML")]
    body_html: String,
    #[serde(flatten)]
    minimized: MinimizedWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentsWire {
    total_count: u32,
    nodes: Vec<Option<CommentWire>>,
}

/// The answer to [`CONVERSATION_FIELDS`].
#[derive(Deserialize)]
pub(super) struct IssueWire {
    #[serde(rename = "bodyHTML")]
    body_html: String,
    #[serde(rename = "createdAt", with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    comments: CommentsWire,
}

impl From<IssueWire> for IssueContent {
    fn from(wire: IssueWire) -> Self {
        let comments: Vec<IssueComment> = wire
            .comments
            .nodes
            .into_iter()
            .flatten()
            .map(|comment| IssueComment {
                author: comment.author.map(|person| person.login),
                created_at: comment.created_at,
                body_html: comment.body_html,
                hidden: comment.minimized.reason(),
            })
            .collect();
        let held = u32::try_from(comments.len()).unwrap_or(u32::MAX);
        Self {
            body_html: wire.body_html,
            comments,
            earlier_comments: wire.comments.total_count.saturating_sub(held),
            opened_at: Some(wire.created_at),
        }
    }
}

impl GitHub {
    /// The description and recent comments of Issue `number`.
    pub fn issue_content(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<IssueContent, Error> {
        let (wire, _points): (IssueWire, RateLimit) =
            self.conversation(Thread::Issue, repository, number)?;
        Ok(wire.into())
    }

    /// The content of `thread` `number`, as `W` reads it, and the points GitHub has left.
    pub(super) fn conversation<W: DeserializeOwned>(
        &self,
        thread: Thread,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<(W, RateLimit), Error> {
        #[derive(Deserialize)]
        #[serde(bound = "W: DeserializeOwned", rename_all = "camelCase")]
        struct Data<W> {
            repository: RepositoryWire<W>,
            rate_limit: RateLimit,
        }
        #[derive(Deserialize)]
        #[serde(bound = "W: DeserializeOwned")]
        struct RepositoryWire<W> {
            #[serde(alias = "pullRequest")]
            issue: Option<W>,
        }
        let data: Data<W> = self.query(&Request {
            operation: thread.operation(),
            query: thread.query().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
            }),
        })?;
        let read = data.repository.issue.ok_or_else(|| {
            Error::new(
                ErrorKind::NotFound,
                &format!("GitHub has no {} #{number} in {repository}.", thread.noun()),
            )
        })?;
        Ok((read, data.rate_limit))
    }
}

#[cfg(test)]
mod tests;
