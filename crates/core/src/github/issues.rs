//! The Issues that PRs name, fetched by number, since GitHub has no search for a list of numbers. A number
//! can turn out to be a PR or to name nothing, and then Tsugiki keeps nothing for it (ADR 0011). An Issue's
//! cross-references list the PRs that may belong in its tree.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use time::OffsetDateTime;

use super::activity::{IssueTimelineWire, issue_activity};
use super::item::alias;
use super::labels::{LabelWire, label_names};
use super::pull_requests::{Nodes, PageRequest, Person, RateLimit};
use super::references::{REFERENCE_FIELDS, ReferenceTimeline};
use super::{GitHub, RATE_LIMIT_FIELDS, Refused, Request};
use crate::error::Error;
use crate::facts::activity::ItemActivity;
use crate::facts::issues::{IssueFacts, IssueState, ListedPr};
use crate::repository::RepositoryName;

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StateWire {
    Open,
    Closed,
}

/// Why GitHub says an Issue closed, or reopened. A reason GitHub adds later reads as a plain close.
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum StateReason {
    NotPlanned,
    Duplicate,
    #[serde(other)]
    Other,
}

impl IssueState {
    fn of(state: StateWire, reason: Option<StateReason>) -> Self {
        match (state, reason) {
            (StateWire::Open, _) => Self::Open,
            (StateWire::Closed, Some(StateReason::NotPlanned)) => Self::NotPlanned,
            (StateWire::Closed, Some(StateReason::Duplicate)) => Self::Duplicate,
            (StateWire::Closed, _) => Self::Closed,
        }
    }
}

/// One answer to a fetch by number.
#[derive(Debug)]
pub struct FetchedIssues {
    /// One for each number that names an Issue.
    pub issues: Vec<IssueFacts>,
    pub activity: Vec<ItemActivity>,
    /// The numbers asked for that will never name an Issue here: GitHub answered a PR, or an Issue under
    /// another number, as it does for one transferred to another repository. A number GitHub doesn't find
    /// isn't among them, since its Issue can still be opened or restored.
    pub issueless: Vec<u32>,
    /// The numbers GitHub refused alone while it answered the others, each logged with what GitHub said:
    /// neither an Issue nor issueless, so a later fetch can ask again.
    pub refused: Vec<u32>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IssueWire {
    number: u32,
    title: String,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
    author: Option<Person>,
    assignees: Nodes<Person>,
    labels: Option<Nodes<LabelWire>>,
    state: StateWire,
    state_reason: Option<StateReason>,
    timeline_items: Nodes<IssueTimelineWire>,
}

impl From<IssueWire> for (IssueFacts, ItemActivity) {
    fn from(wire: IssueWire) -> Self {
        let activity = issue_activity(wire.number, wire.timeline_items);
        let facts = IssueFacts {
            number: wire.number,
            title: wire.title,
            author: wire.author.map(|person| person.login),
            assignees: wire
                .assignees
                .nodes
                .into_iter()
                .flatten()
                .map(|person| person.login)
                .collect(),
            labels: label_names(wire.labels),
            updated_at: wire.updated_at,
            state: Some(IssueState::of(wire.state, wire.state_reason)),
        };
        (facts, activity)
    }
}

/// The fields of an Issue that `IssueWire` reads. The last comments and mentions are enough to say that
/// someone posted, and whether the post named the user. An assignment's moment says whether it started after
/// the user muted the Issue (#492).
pub(super) const ISSUE_FIELDS: &str = "number title updatedAt state stateReason author { login } \
     assignees(first: 20) { nodes { login } } labels(first: 100) { nodes { name } } \
     timelineItems(last: 50, itemTypes: [ISSUE_COMMENT, MENTIONED_EVENT, ASSIGNED_EVENT]) { nodes { \
     __typename \
     ... on IssueComment { createdAt author { __typename login } } \
     ... on MentionedEvent { createdAt actor { __typename login } } \
     ... on AssignedEvent { createdAt assignee { ... on User { login } ... on Bot { login } } } } }";

/// One field for each of `numbers`, each asking `fields` of an Issue, because GraphQL has no way to look up a
/// list of numbers and a `$` variable can't add a field. Only digits reach this text.
pub(super) fn query_by_number(operation: &str, numbers: &[u32], fields: &str) -> String {
    let asked: Vec<String> = numbers
        .iter()
        .map(|number| {
            format!(
                "{}: issueOrPullRequest(number: {number}) {{ __typename ... on Issue {{ {fields} }} }}",
                alias(*number)
            )
        })
        .collect();
    format!(
        "query {operation}($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) \
         {{ {} }} {RATE_LIMIT_FIELDS} }}",
        asked.join(" ")
    )
}

fn fetch_issues_query(numbers: &[u32]) -> String {
    query_by_number("FetchIssues", numbers, ISSUE_FIELDS)
}

/// The request of a read of Issues by number.
pub(super) struct ByNumber<'a> {
    pub operation: &'static str,
    pub query: fn(&[u32]) -> String,
    /// At least one.
    pub numbers: &'a [u32],
    pub size: usize,
}

/// What a read of Issues by number reads of one Issue, which says the number it came back under.
pub(super) trait AnsweredIssue {
    fn number(&self) -> u32;
}

impl AnsweredIssue for IssueWire {
    fn number(&self) -> u32 {
        self.number
    }
}

/// An answer to a read of Issues by number.
pub(super) struct ReadByNumber<T> {
    /// One for each number that names an Issue.
    pub issues: Vec<T>,
    /// The numbers asked for that will never name an Issue here: GitHub answered a PR, or an Issue under
    /// another number, as it does for one transferred to another repository. A number GitHub doesn't find
    /// isn't among them, since its Issue can still be opened or restored.
    pub issueless: Vec<u32>,
    /// The numbers GitHub refused alone while it answered the others: neither an Issue nor issueless.
    pub refused: Vec<u32>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

/// One page of an Issue's cross-references, the newest PR first.
#[derive(Debug, PartialEq, Eq)]
pub struct CrossReferences {
    /// Only PRs of this Repository: a PR elsewhere can't fix this Issue (ADR 0011).
    pub listed: Vec<ListedPr>,
    /// The next page's cursor, which holds older events, if there is one.
    pub next: Option<String>,
    pub rate_limit: RateLimit,
}

/// GitHub lists events oldest first, so `last` and `before` read them from the newest (ADR 0011).
fn fetch_cross_references_query() -> String {
    format!(
        "query FetchCrossReferences($owner: String!, $name: String!, $number: Int!, $last: Int!, \
         $before: String) {{ repository(owner: $owner, name: $name) {{ issue(number: $number) {{ \
         timelineItems(last: $last, before: $before, itemTypes: [CROSS_REFERENCED_EVENT]) {{ \
         {REFERENCE_FIELDS} }} }} }} {RATE_LIMIT_FIELDS} }}"
    )
}

impl GitHub {
    /// One page of the cross-references of Issue `number`, or half that page after a 502 or no answer. An
    /// Issue GitHub no longer has here, as after a transfer, has none.
    pub fn fetch_cross_references(
        &self,
        repository: &RepositoryName,
        number: u32,
        page: PageRequest<'_>,
    ) -> Result<CrossReferences, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Issue {
            timeline_items: ReferenceTimeline,
        }
        #[derive(Deserialize)]
        struct Holder {
            issue: Option<Issue>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Holder,
            rate_limit: RateLimit,
        }
        let (data, _): (Data, usize) = self.query_at_size(page.size, |last| Request {
            operation: "FetchCrossReferences",
            query: fetch_cross_references_query().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
                "last": last,
                "before": page.after,
            }),
        })?;
        let Some(issue) = data.repository.issue else {
            return Ok(CrossReferences {
                listed: Vec::new(),
                next: None,
                rate_limit: data.rate_limit,
            });
        };
        let (listed, next) = issue.timeline_items.read(repository);
        Ok(CrossReferences {
            listed,
            next,
            rate_limit: data.rate_limit,
        })
    }

    /// The first `size` of `numbers`, or half that after a 502 or no answer. `numbers` holds at least one.
    pub fn fetch_issues(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<FetchedIssues, Error> {
        let read: ReadByNumber<IssueWire> = self.read_by_number(
            repository,
            ByNumber {
                operation: "FetchIssues",
                query: fetch_issues_query,
                numbers,
                size,
            },
        )?;
        let (issues, activity) = read
            .issues
            .into_iter()
            .map(<(IssueFacts, ItemActivity)>::from)
            .unzip();
        Ok(FetchedIssues {
            issues,
            activity,
            issueless: read.issueless,
            refused: read.refused,
            asked_for: read.asked_for,
            rate_limit: read.rate_limit,
        })
    }

    /// Asks for the first `read.size` of `read.numbers`, or half that after a 502 or no answer. A number can
    /// turn out to be a PR or a transferred Issue, and then the answer counts it as issueless. GitHub can
    /// refuse one number, as with a FORBIDDEN for its alias alone, and answer the others: that number is
    /// logged and given back as refused, and the rest of the answer stands.
    pub(super) fn read_by_number<T: DeserializeOwned + AnsweredIssue>(
        &self,
        repository: &RepositoryName,
        read: ByNumber<'_>,
    ) -> Result<ReadByNumber<T>, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "__typename", bound = "T: DeserializeOwned")]
        enum Numbered<T> {
            Issue(T),
            PullRequest,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", bound = "T: DeserializeOwned")]
        struct Data<T> {
            repository: BTreeMap<String, Option<Numbered<T>>>,
            rate_limit: RateLimit,
        }
        let ByNumber {
            operation,
            query,
            numbers,
            size,
        } = read;
        let aliases: Vec<String> = numbers.iter().map(|number| alias(*number)).collect();
        let refusable = |field: &str| aliases.iter().any(|asked| asked == field);
        let ((data, refused), asked_for): ((Data<T>, Vec<Refused>), usize) = self.read_at_size(
            size.min(numbers.len()),
            |size| {
                let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
                Request {
                    operation,
                    query: query(&asked).into(),
                    variables: json!({ "owner": repository.owner(), "name": repository.name() }),
                }
            },
            |reply| self.answer_with_refused(reply, refusable),
        )?;
        let mut answers = data.repository;
        let mut issues = Vec::new();
        let mut issueless = Vec::new();
        let mut refused_numbers = Vec::new();
        for number in numbers.iter().take(asked_for) {
            let field = alias(*number);
            if let Some(refusal) = refused.iter().find(|refusal| refusal.field == field) {
                log::warn!("GitHub wouldn't give Issue {number}: {}", refusal.message);
                refused_numbers.push(*number);
                continue;
            }
            // GitHub follows an Issue transferred to another repository, and answers with the number it has
            // there. `read` lets a null through only with GitHub's NOT_FOUND for it, which can change: the
            // Issue a PR names can be opened later, or one GitHub hid be restored.
            match answers.remove(&field) {
                Some(Some(Numbered::Issue(issue))) if issue.number() == *number => {
                    issues.push(issue)
                }
                Some(Some(Numbered::Issue(_) | Numbered::PullRequest)) => issueless.push(*number),
                Some(None) | None => {}
            }
        }
        Ok(ReadByNumber {
            issues,
            issueless,
            refused: refused_numbers,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
