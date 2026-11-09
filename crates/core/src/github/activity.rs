//! The events on an Issue or a PR, parsed from GitHub's timeline. A bot's events stop here, apart from the
//! comment that says a Bot backport failed.

use serde::Deserialize;
use time::OffsetDateTime;

use super::pull_requests::{Nodes, Person, ReviewerWire};
use crate::conventions::Conventions;
use crate::login::Login;

/// The name GitHub itself gives an account that no longer exists.
const NO_ACCOUNT: &str = "ghost";
/// The committer of what someone does in GitHub's web pages. The commit's author is the person.
pub(super) const WEB_COMMITTER: &str = "web-flow";
/// The end of the name a GitHub App commits under. Those commits have no user to look at.
pub(super) const APP_NAME_END: &str = "[bot]";

/// The kinds of thing a person, or a backport bot's failure, adds to a timeline.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActivityKind {
    /// A pushed commit or a rewritten history.
    Commit,
    Comment,
    Review,
    /// A Draft PR became ready for review.
    ReadyForReview,
    /// The backport bot said it couldn't backport this PR to `target`.
    BackportFailed {
        target: String,
    },
    /// Someone asked `who` to review this PR. It says when a Review request began, and it's no news itself.
    ReviewRequested,
    /// Someone assigned this PR to `who`. It says when a Review request began, and it's no news itself.
    Assigned,
    /// A post mentioned `who`, the person GitHub names on this event. `who` didn't write the post.
    Mentioned,
}

/// One thing that happened on an Issue or a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub who: Login,
    pub at: OffsetDateTime,
}

/// The activity GitHub reported on one Issue or PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemActivity {
    pub number: u32,
    pub events: Vec<Activity>,
}

#[derive(Deserialize)]
enum ActorKind {
    Bot,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub(super) struct ActorWire {
    #[serde(rename = "__typename")]
    kind: ActorKind,
    login: Login,
}

/// The person behind `actor`, or nothing for a bot. An account that has gone is still a person.
fn person(actor: Option<ActorWire>) -> Option<Login> {
    match actor {
        None => Some(Login::new(NO_ACCOUNT)),
        Some(ActorWire {
            kind: ActorKind::Bot,
            ..
        }) => None,
        Some(ActorWire {
            kind: ActorKind::Other,
            login,
        }) => Some(login),
    }
}

#[derive(Deserialize)]
pub(super) struct GitActorWire {
    name: Option<String>,
    user: Option<Person>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelineCommitWire {
    #[serde(with = "time::serde::rfc3339")]
    committed_date: OffsetDateTime,
    author: Option<GitActorWire>,
    committer: Option<GitActorWire>,
}

impl TimelineCommitWire {
    /// The person who put this commit on the PR, or nothing for an App. A Backport's commits keep their
    /// original authors, so the committer speaks for who pushed, unless GitHub's web pages made the commit or
    /// the committer's email names no account.
    fn pusher(self) -> Option<Login> {
        let is_app = |actor: &Option<GitActorWire>| {
            let actor = actor.as_ref();
            let name = actor.and_then(|actor| actor.name.as_ref());
            name.is_some_and(|name| name.ends_with(APP_NAME_END))
        };
        if is_app(&self.committer) {
            return None;
        }
        let account = |actor: Option<GitActorWire>| {
            let user = actor.and_then(|actor| actor.user);
            let login = user.map(|user| user.login);
            login.filter(|login| *login != Login::new(WEB_COMMITTER))
        };
        if let Some(committer) = account(self.committer) {
            return Some(committer);
        }
        let app = is_app(&self.author);
        match account(self.author) {
            Some(author) => Some(author),
            None if app => None,
            None => Some(Login::new(NO_ACCOUNT)),
        }
    }
}

/// One item of a PR's timeline, of the kinds `PULL_REQUEST_FIELDS` asks for.
#[derive(Deserialize)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
pub(super) enum TimelineWire {
    PullRequestCommit {
        commit: TimelineCommitWire,
    },
    IssueComment {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        author: Option<ActorWire>,
        body_text: String,
    },
    PullRequestReview {
        /// Nothing, for a review its author hasn't submitted.
        #[serde(with = "time::serde::rfc3339::option")]
        submitted_at: Option<OffsetDateTime>,
        author: Option<ActorWire>,
    },
    ReadyForReviewEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<ActorWire>,
    },
    HeadRefForcePushedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<ActorWire>,
    },
    MentionedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<ActorWire>,
    },
    ReviewRequestedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        requested_reviewer: Option<ReviewerWire>,
    },
    AssignedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        assignee: Option<ReviewerWire>,
    },
}

impl TimelineWire {
    fn activity(self, conventions: Conventions) -> Option<Activity> {
        let by_person =
            |kind, who: Option<ActorWire>, at| person(who).map(|who| Activity { kind, who, at });
        match self {
            Self::PullRequestCommit { commit } => {
                let at = commit.committed_date;
                let who = commit.pusher()?;
                let kind = ActivityKind::Commit;
                Some(Activity { kind, who, at })
            }
            Self::IssueComment {
                created_at,
                author,
                body_text,
            } => {
                let failed = author.as_ref().and_then(|author| {
                    let target = conventions.failed_backport_target(&author.login, &body_text)?;
                    Some((target, author.login.clone()))
                });
                match failed {
                    Some((target, who)) => Some(Activity {
                        kind: ActivityKind::BackportFailed { target },
                        who,
                        at: created_at,
                    }),
                    None => by_person(ActivityKind::Comment, author, created_at),
                }
            }
            Self::PullRequestReview {
                submitted_at,
                author,
            } => by_person(ActivityKind::Review, author, submitted_at?),
            Self::ReadyForReviewEvent { created_at, actor } => {
                by_person(ActivityKind::ReadyForReview, actor, created_at)
            }
            Self::HeadRefForcePushedEvent { created_at, actor } => {
                by_person(ActivityKind::Commit, actor, created_at)
            }
            Self::MentionedEvent { created_at, actor } => {
                by_person(ActivityKind::Mentioned, actor, created_at)
            }
            Self::ReviewRequestedEvent {
                created_at,
                requested_reviewer,
            } => Some(Activity {
                kind: ActivityKind::ReviewRequested,
                who: requested_reviewer?.login?,
                at: created_at,
            }),
            Self::AssignedEvent {
                created_at,
                assignee,
            } => Some(Activity {
                kind: ActivityKind::Assigned,
                who: assignee?.login?,
                at: created_at,
            }),
        }
    }
}

/// The activity in PR `number`'s `timeline`.
pub(super) fn pull_request_activity(
    number: u32,
    timeline: Nodes<TimelineWire>,
    conventions: Conventions,
) -> ItemActivity {
    let items = timeline.nodes.into_iter().flatten();
    ItemActivity {
        number,
        events: items
            .filter_map(|item| item.activity(conventions))
            .collect(),
    }
}

/// One item of an Issue's timeline, of the kinds `ISSUE_FIELDS` asks for.
#[derive(Deserialize)]
#[serde(tag = "__typename", rename_all_fields = "camelCase")]
pub(super) enum IssueTimelineWire {
    IssueComment {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        author: Option<ActorWire>,
    },
    MentionedEvent {
        #[serde(with = "time::serde::rfc3339")]
        created_at: OffsetDateTime,
        actor: Option<ActorWire>,
    },
}

/// The activity in Issue `number`'s `timeline`.
pub(super) fn issue_activity(number: u32, timeline: Nodes<IssueTimelineWire>) -> ItemActivity {
    let items = timeline.nodes.into_iter().flatten();
    let events = items.filter_map(|item| {
        let (kind, who, at) = match item {
            IssueTimelineWire::IssueComment { created_at, author } => {
                (ActivityKind::Comment, author, created_at)
            }
            IssueTimelineWire::MentionedEvent { created_at, actor } => {
                (ActivityKind::Mentioned, actor, created_at)
            }
        };
        let who = person(who)?;
        Some(Activity { kind, who, at })
    });
    ItemActivity {
        number,
        events: events.collect(),
    }
}

#[cfg(test)]
mod tests;
