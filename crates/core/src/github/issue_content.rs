//! The description and recent comments of one Issue or PR, as GitHub renders them. Only the Issue view and
//! the PR view's Conversation show them, so Tsugiki asks when one opens and while it's on screen. A refresh
//! reads ahead only the Conversations of the PRs in Reach (ADR 0015).

use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use time::OffsetDateTime;

use super::issues::StateReason;
use super::labels::LabelWire;
use super::pull_requests::{Person, RateLimit};
use super::references::NamedRepository;
use super::{GitHub, RATE_LIMIT_FIELDS, Request};
use crate::error::{Error, ErrorKind};
use crate::facts::issue_content::{
    Comment, CommentId, CommitReference, HiddenReason, IssueComment, IssueContent, IssueEvent,
    IssueEventKind, Reference,
};
use crate::facts::pull_requests::NodeId;
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
            Self::Issue => EVENTS_FIELDS,
            Self::PullRequest => super::pull_request_content::REVIEW_FIELDS,
        }
    }

    pub(super) fn query(self) -> String {
        format!(
            "query {}($owner: String!, $name: String!, $number: Int!) {{ \
             repository(owner: $owner, name: $name) {{ {}(number: $number) {{ {CONVERSATION_FIELDS}{} }} }} \
             {RATE_LIMIT_FIELDS} }}",
            self.operation(),
            self.field(),
            self.more_fields(),
        )
    }
}

/// The fields an Issue and a PR share: its ID, which a write names it by, the description, when it was
/// opened, and the last 30 comments.
pub(super) const CONVERSATION_FIELDS: &str = "id bodyHTML createdAt comments(last: 30) { totalCount \
                                              nodes { fullDatabaseId author { login } createdAt bodyHTML \
                                              isMinimized minimizedReason } }";

/// The events the Issue view shows, the last 30 of the kinds it draws, in time order (#1067). They come from
/// `timelineItems`, whose own connection keeps the count of the older ones GitHub still holds.
pub(super) const EVENTS_FIELDS: &str = "events: timelineItems(last: 30, itemTypes: [\
     CROSS_REFERENCED_EVENT, REFERENCED_EVENT, CONNECTED_EVENT, DISCONNECTED_EVENT, LABELED_EVENT, \
     UNLABELED_EVENT, ASSIGNED_EVENT, UNASSIGNED_EVENT, CLOSED_EVENT, REOPENED_EVENT, RENAMED_TITLE_EVENT]) \
     { totalCount nodes { __typename \
     ... on CrossReferencedEvent { createdAt actor { login } source { __typename \
       ... on Issue { number title url repository { nameWithOwner } } \
       ... on PullRequest { number title url repository { nameWithOwner } } } } \
     ... on ReferencedEvent { createdAt actor { login } commit { abbreviatedOid messageHeadline url } } \
     ... on ConnectedEvent { createdAt actor { login } source { __typename \
       ... on Issue { number title url repository { nameWithOwner } } \
       ... on PullRequest { number title url repository { nameWithOwner } } } subject { __typename \
       ... on Issue { number title url repository { nameWithOwner } } \
       ... on PullRequest { number title url repository { nameWithOwner } } } } \
     ... on DisconnectedEvent { createdAt actor { login } source { __typename \
       ... on Issue { number title url repository { nameWithOwner } } \
       ... on PullRequest { number title url repository { nameWithOwner } } } subject { __typename \
       ... on Issue { number title url repository { nameWithOwner } } \
       ... on PullRequest { number title url repository { nameWithOwner } } } } \
     ... on LabeledEvent { createdAt actor { login } label { name } } \
     ... on UnlabeledEvent { createdAt actor { login } label { name } } \
     ... on AssignedEvent { createdAt actor { login } assignee { ... on User { login } \
       ... on Bot { login } ... on Organization { login } ... on Mannequin { login } } } \
     ... on UnassignedEvent { createdAt actor { login } assignee { ... on User { login } \
       ... on Bot { login } ... on Organization { login } ... on Mannequin { login } } } \
     ... on ClosedEvent { createdAt actor { login } stateReason } \
     ... on ReopenedEvent { createdAt actor { login } } \
     ... on RenamedTitleEvent { createdAt actor { login } previousTitle } } }";

/// GitHub's `BigInt`, such as a `fullDatabaseId`, which it sends as a string of digits. Its `databaseId` is
/// a 32-bit `Int`, too small for the numbers of comments and Reviews today. Nothing, for null.
pub(super) fn big_int<'de, D: Deserializer<'de>>(answer: D) -> Result<Option<u64>, D::Error> {
    let Some(digits) = Option::<String>::deserialize(answer)? else {
        return Ok(None);
    };
    let number = digits.parse().map_err(|cause| {
        D::Error::custom(format!(
            "GitHub's BigInt {digits:?} isn't a number: {cause}"
        ))
    })?;
    Ok(Some(number))
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommentWire {
    #[serde(default, deserialize_with = "big_int")]
    full_database_id: Option<u64>,
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

/// A PR or an Issue that kind of event points at.
#[derive(Deserialize)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
enum SubjectWire {
    Issue {
        number: u32,
        title: String,
        url: String,
        repository: NamedRepository,
    },
    PullRequest {
        number: u32,
        title: String,
        url: String,
        repository: NamedRepository,
    },
}

impl SubjectWire {
    fn reference(self) -> Reference {
        let (number, title, url, repository) = match self {
            Self::Issue {
                number,
                title,
                url,
                repository,
            }
            | Self::PullRequest {
                number,
                title,
                url,
                repository,
            } => (number, title, url, repository),
        };
        Reference {
            repository: repository.name_with_owner,
            number,
            title,
            url,
        }
    }

    fn is_pull_request(&self) -> bool {
        matches!(self, Self::PullRequest { .. })
    }
}

/// The PR of a hand link. GitHub puts it on the event's subject, but its source may hold it too, so this
/// takes the PR of the two.
fn linked(source: SubjectWire, subject: SubjectWire) -> Reference {
    if source.is_pull_request() {
        source.reference()
    } else if subject.is_pull_request() {
        subject.reference()
    } else {
        source.reference()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitReferenceWire {
    abbreviated_oid: String,
    message_headline: String,
    url: String,
}

/// One item of the Issue's `timelineItems`, of the kinds [`EVENTS_FIELDS`] asks for.
#[derive(Deserialize)]
#[expect(
    clippy::enum_variant_names,
    reason = "GitHub's `__typename` names each kind, and each ends with Event"
)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
enum EventWire {
    CrossReferencedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        source: SubjectWire,
    },
    ReferencedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        commit: Option<CommitReferenceWire>,
    },
    ConnectedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        source: SubjectWire,
        subject: SubjectWire,
    },
    DisconnectedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        source: SubjectWire,
        subject: SubjectWire,
    },
    LabeledEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        label: LabelWire,
    },
    UnlabeledEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        label: LabelWire,
    },
    AssignedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        assignee: Option<Person>,
    },
    UnassignedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        assignee: Option<Person>,
    },
    ClosedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        state_reason: Option<StateReason>,
    },
    ReopenedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
    },
    RenamedTitleEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<Person>,
        previous_title: String,
    },
}

impl EventWire {
    fn event(self) -> Option<IssueEvent> {
        let (created_at, actor, kind) = match self {
            Self::CrossReferencedEvent {
                created_at,
                actor,
                source,
            } => (
                created_at,
                actor,
                IssueEventKind::CrossReferenced {
                    reference: source.reference(),
                },
            ),
            Self::ReferencedEvent {
                created_at,
                actor,
                commit,
            } => {
                // The commit a reference named can have gone, and the line has nothing to say without it.
                let commit = commit?;
                (
                    created_at,
                    actor,
                    IssueEventKind::Referenced {
                        commit: CommitReference {
                            sha: commit.abbreviated_oid,
                            message: commit.message_headline,
                            url: commit.url,
                        },
                    },
                )
            }
            Self::ConnectedEvent {
                created_at,
                actor,
                source,
                subject,
            } => (
                created_at,
                actor,
                IssueEventKind::Connected {
                    reference: linked(source, subject),
                },
            ),
            Self::DisconnectedEvent {
                created_at,
                actor,
                source,
                subject,
            } => (
                created_at,
                actor,
                IssueEventKind::Disconnected {
                    reference: linked(source, subject),
                },
            ),
            Self::LabeledEvent {
                created_at,
                actor,
                label,
            } => (
                created_at,
                actor,
                IssueEventKind::Labeled { label: label.name },
            ),
            Self::UnlabeledEvent {
                created_at,
                actor,
                label,
            } => (
                created_at,
                actor,
                IssueEventKind::Unlabeled { label: label.name },
            ),
            Self::AssignedEvent {
                created_at,
                actor,
                assignee,
            } => (
                created_at,
                actor,
                IssueEventKind::Assigned {
                    assignee: assignee.map(|assignee| assignee.login),
                },
            ),
            Self::UnassignedEvent {
                created_at,
                actor,
                assignee,
            } => (
                created_at,
                actor,
                IssueEventKind::Unassigned {
                    assignee: assignee.map(|assignee| assignee.login),
                },
            ),
            Self::ClosedEvent {
                created_at,
                actor,
                state_reason,
            } => (
                created_at,
                actor,
                IssueEventKind::Closed {
                    completed: !matches!(
                        state_reason,
                        Some(StateReason::NotPlanned | StateReason::Duplicate)
                    ),
                },
            ),
            Self::ReopenedEvent { created_at, actor } => {
                (created_at, actor, IssueEventKind::Reopened)
            }
            Self::RenamedTitleEvent {
                created_at,
                actor,
                previous_title,
            } => (
                created_at,
                actor,
                IssueEventKind::Renamed { previous_title },
            ),
        };
        Some(IssueEvent {
            actor: actor.map(|actor| actor.login),
            created_at,
            kind,
        })
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct EventsWire {
    total_count: u32,
    #[serde(default)]
    nodes: Vec<Option<EventWire>>,
}

/// The answer to [`CONVERSATION_FIELDS`].
#[derive(Deserialize)]
pub(super) struct IssueWire {
    id: NodeId,
    #[serde(rename = "bodyHTML")]
    body_html: String,
    #[serde(rename = "createdAt", with = "time::serde::rfc3339")]
    created_at: OffsetDateTime,
    comments: CommentsWire,
    /// A PR's query asks for no events, so its Conversation keeps none until an Issue's ticket adds them.
    #[serde(default)]
    events: EventsWire,
}

impl From<IssueWire> for IssueContent {
    fn from(wire: IssueWire) -> Self {
        let comments: Vec<IssueComment> = wire
            .comments
            .nodes
            .into_iter()
            .flatten()
            .map(|comment| IssueComment {
                id: comment.full_database_id.map(CommentId::new),
                comment: Comment {
                    author: comment.author.map(|person| person.login),
                    created_at: comment.created_at,
                    body_html: comment.body_html,
                    hidden: comment.minimized.reason(),
                },
            })
            .collect();
        let held = u32::try_from(comments.len()).unwrap_or(u32::MAX);
        // A referenced event whose commit GitHub no longer has is dropped, but it was still one of the
        // events this read brought, so it doesn't count as an older one.
        let fetched_events = wire.events.nodes.iter().flatten().count();
        let events: Vec<IssueEvent> = wire
            .events
            .nodes
            .into_iter()
            .flatten()
            .filter_map(EventWire::event)
            .collect();
        let held_events = u32::try_from(fetched_events).unwrap_or(u32::MAX);
        Self {
            id: Some(wire.id),
            body_html: wire.body_html,
            comments,
            earlier_comments: wire.comments.total_count.saturating_sub(held),
            events,
            earlier_events: wire.events.total_count.saturating_sub(held_events),
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
