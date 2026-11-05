//! The events on an Issue or a PR, parsed from GitHub's timeline. A bot's events stop here, apart from the
//! comment that says a Bot backport failed.

use serde::Deserialize;
use time::OffsetDateTime;

use super::Login;
use super::pull_requests::{Nodes, Person, ReviewerWire};
use crate::conventions::Conventions;

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
mod tests {
    use serde_json::{Value, json};
    use time::format_description::well_known::Rfc3339;

    use super::*;

    const AT: &str = "2026-11-04T10:00:00Z";

    fn actor(kind: &str, login: &str) -> Value {
        json!({ "__typename": kind, "login": login })
    }

    fn user(login: &str) -> Value {
        json!({ "name": login, "user": { "login": login } })
    }

    fn commit(author: &Value, committer: &Value) -> Value {
        json!({
            "__typename": "PullRequestCommit",
            "commit": { "committedDate": AT, "author": author, "committer": committer },
        })
    }

    fn comment(author: &Value, text: &str) -> Value {
        json!({ "__typename": "IssueComment", "createdAt": AT, "author": author, "bodyText": text })
    }

    fn activity(item: Value, conventions: Conventions) -> ItemActivity {
        let timeline = serde_json::from_value(json!({ "nodes": [item] })).unwrap();
        pull_request_activity(7, timeline, conventions)
    }

    fn one(kind: ActivityKind, who: &str) -> Vec<Activity> {
        vec![Activity {
            kind,
            who: Login::new(who),
            at: OffsetDateTime::parse(AT, &Rfc3339).unwrap(),
        }]
    }

    #[test]
    fn a_commit_belongs_to_whoever_pushed_it() {
        let read = activity(
            commit(&user("original-author"), &user("some-author")),
            Conventions::Github,
        );
        assert_eq!(read.number, 7);
        assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
    }

    #[test]
    fn a_commit_made_in_githubs_web_pages_belongs_to_its_author() {
        let read = activity(
            commit(&user("some-author"), &user("web-flow")),
            Conventions::Github,
        );
        assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
    }

    #[test]
    fn a_commit_with_no_committer_belongs_to_its_author() {
        let read = activity(
            commit(&user("some-author"), &Value::Null),
            Conventions::Github,
        );
        assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
    }

    #[test]
    fn a_commit_with_no_account_still_counts_as_someone() {
        let nobody = json!({ "name": "Someone", "user": null });
        let read = activity(commit(&nobody, &nobody), Conventions::Github);
        assert_eq!(read.events, one(ActivityKind::Commit, "ghost"));
        let read = activity(commit(&Value::Null, &Value::Null), Conventions::Github);
        assert_eq!(read.events, one(ActivityKind::Commit, "ghost"));
    }

    #[test]
    fn a_committers_email_with_no_account_leaves_the_commit_to_its_author() {
        let nobody = json!({ "name": "Someone", "user": null });
        let read = activity(commit(&user("some-author"), &nobody), Conventions::Github);
        assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
    }

    #[test]
    fn an_app_committing_for_a_person_doesnt_count() {
        let app = json!({ "name": "dependabot[bot]", "user": null });
        let read = activity(commit(&user("some-author"), &app), Conventions::Github);
        assert_eq!(read.events, []);
    }

    #[test]
    fn an_apps_commit_through_githubs_web_pages_doesnt_count() {
        let app = json!({ "name": "dependabot[bot]", "user": null });
        let read = activity(commit(&app, &user("web-flow")), Conventions::Github);
        assert_eq!(read.events, []);
    }

    #[test]
    fn an_apps_commit_doesnt_count() {
        let app = json!({ "name": "dependabot[bot]", "user": null });
        let read = activity(commit(&app, &app), Conventions::Github);
        assert_eq!(read.events, []);
    }

    #[test]
    fn a_bots_comment_doesnt_count() {
        let read = activity(
            comment(&actor("Bot", "bedevere-app"), "GH-2 is a backport"),
            Conventions::Cpython,
        );
        assert_eq!(read.events, []);
    }

    #[test]
    fn a_persons_comment_counts_and_so_does_one_from_an_account_that_has_gone() {
        let read = activity(
            comment(&actor("User", "some-reviewer"), "Looks good"),
            Conventions::Cpython,
        );
        assert_eq!(read.events, one(ActivityKind::Comment, "some-reviewer"));
        let read = activity(comment(&Value::Null, "Looks good"), Conventions::Cpython);
        assert_eq!(read.events, one(ActivityKind::Comment, "ghost"));
    }

    #[test]
    fn the_bots_conflict_comment_says_a_bot_backport_failed() {
        let text = "Sorry, @some-author and @some-reviewer, I could not cleanly backport this to \
                    3.14 due to a conflict.\nPlease backport manually with cherry_picker.";
        let read = activity(
            comment(&actor("Bot", "miss-islington-app"), text),
            Conventions::Cpython,
        );
        let kind = ActivityKind::BackportFailed {
            target: "3.14".to_owned(),
        };
        assert_eq!(read.events, one(kind, "miss-islington-app"));
    }

    #[test]
    fn a_repository_with_no_backport_bot_reads_those_words_as_any_bots_comment() {
        let text = "I could not cleanly backport this to 3.14 due to a conflict.";
        let read = activity(
            comment(&actor("Bot", "miss-islington-app"), text),
            Conventions::Github,
        );
        assert_eq!(read.events, []);
    }

    #[test]
    fn a_submitted_review_counts_and_a_pending_one_doesnt() {
        let review = |submitted: &Value| {
            json!({
                "__typename": "PullRequestReview",
                "submittedAt": submitted,
                "author": actor("User", "some-reviewer"),
            })
        };
        let read = activity(review(&json!(AT)), Conventions::Github);
        assert_eq!(read.events, one(ActivityKind::Review, "some-reviewer"));
        let read = activity(review(&Value::Null), Conventions::Github);
        assert_eq!(read.events, []);
    }

    #[test]
    fn ready_for_review_and_a_rewritten_history_count() {
        let event = |kind: &str| json!({ "__typename": kind, "createdAt": AT, "actor": actor("User", "some-author") });
        let read = activity(event("ReadyForReviewEvent"), Conventions::Github);
        assert_eq!(
            read.events,
            one(ActivityKind::ReadyForReview, "some-author")
        );
        let read = activity(event("HeadRefForcePushedEvent"), Conventions::Github);
        assert_eq!(read.events, one(ActivityKind::Commit, "some-author"));
    }

    #[test]
    fn an_issues_comments_count_unless_a_bot_wrote_them() {
        let comments = json!({ "nodes": [
            { "__typename": "IssueComment", "createdAt": AT, "author": actor("User", "issue-author") },
            { "__typename": "IssueComment", "createdAt": AT, "author": actor("Bot", "bedevere-app") },
            null,
        ] });
        let read = issue_activity(3, serde_json::from_value(comments).unwrap());
        assert_eq!(read.number, 3);
        assert_eq!(read.events, one(ActivityKind::Comment, "issue-author"));
    }

    #[test]
    fn a_mention_of_someone_on_an_issue_names_the_person_it_mentions() {
        let timeline = json!({ "nodes": [
            { "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("User", "issue-author") },
            { "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("Bot", "bedevere-app") },
        ] });
        let read = issue_activity(3, serde_json::from_value(timeline).unwrap());
        assert_eq!(read.events, one(ActivityKind::Mentioned, "issue-author"));
    }

    #[test]
    fn a_review_request_and_an_assignment_on_a_pr_name_the_person_they_ask() {
        let asked = |kind: &str, field: &str| {
            json!({
                "__typename": kind, "createdAt": AT, field: { "login": "some-reviewer" },
            })
        };
        let nodes = |item: Value| json!({ "nodes": [item] });
        let read_one = |item: Value| {
            let timeline = serde_json::from_value(nodes(item)).unwrap();
            pull_request_activity(3, timeline, Conventions::Github).events
        };

        assert_eq!(
            read_one(asked("ReviewRequestedEvent", "requestedReviewer")),
            one(ActivityKind::ReviewRequested, "some-reviewer")
        );
        assert_eq!(
            read_one(asked("AssignedEvent", "assignee")),
            one(ActivityKind::Assigned, "some-reviewer")
        );
        assert_eq!(
            read_one(json!({
                "__typename": "MentionedEvent", "createdAt": AT, "actor": actor("User", "some-reviewer"),
            })),
            one(ActivityKind::Mentioned, "some-reviewer")
        );
    }

    #[test]
    fn a_review_request_of_a_team_and_an_assignee_gone_from_github_name_nobody() {
        let nodes = json!({ "nodes": [
            { "__typename": "ReviewRequestedEvent", "createdAt": AT, "requestedReviewer": {} },
            { "__typename": "ReviewRequestedEvent", "createdAt": AT, "requestedReviewer": null },
            { "__typename": "AssignedEvent", "createdAt": AT, "assignee": {} },
            { "__typename": "AssignedEvent", "createdAt": AT, "assignee": null },
        ] });
        let read = pull_request_activity(
            3,
            serde_json::from_value(nodes).unwrap(),
            Conventions::Github,
        );

        assert_eq!(read.events, []);
    }
}
