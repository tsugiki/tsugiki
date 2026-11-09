//! The Issues that PRs name, fetched by number, since GitHub has no search for a list of numbers. A number
//! can turn out to be a PR or to name nothing, and then Tsugiki keeps nothing for it (ADR 0011). An Issue's
//! cross-references list the PRs that may belong in its tree.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::activity::{IssueTimelineWire, ItemActivity, issue_activity};
use super::pull_requests::{NodeId, Nodes, PageRequest, Person, RateLimit};
use super::references::{REFERENCE_FIELDS, ReferenceTimeline};
use super::{GitHub, Request};
use crate::error::Error;
use crate::login::Login;
use crate::repository::RepositoryName;

/// An Issue a PR names, as Tsugiki keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFacts {
    pub number: u32,
    pub title: String,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    pub assignees: Vec<Login>,
    pub updated_at: OffsetDateTime,
    /// Nothing for an Issue stored before Tsugiki kept states, until GitHub sends it again.
    pub state: Option<IssueState>,
}

/// Whether an Issue is open, and how it closed, as GitHub draws it (#188).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueState {
    Open,
    /// Closed as completed, or closed before GitHub said why.
    Closed,
    NotPlanned,
    /// Closed as a duplicate, which GitHub draws as it draws not planned.
    Duplicate,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StateWire {
    Open,
    Closed,
}

/// Why GitHub says an Issue closed, or reopened. A reason GitHub adds later reads as a plain close.
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum StateReason {
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
            updated_at: wire.updated_at,
            state: Some(IssueState::of(wire.state, wire.state_reason)),
        };
        (facts, activity)
    }
}

/// The fields of an Issue that `IssueWire` reads. The last comments and mentions are enough to say that
/// someone posted, and whether the post named the user.
pub(super) const ISSUE_FIELDS: &str = "number title updatedAt state stateReason author { login } \
     assignees(first: 20) { nodes { login } } \
     timelineItems(last: 50, itemTypes: [ISSUE_COMMENT, MENTIONED_EVENT]) { nodes { __typename \
     ... on IssueComment { createdAt author { __typename login } } \
     ... on MentionedEvent { createdAt actor { __typename login } } } }";

/// One field for each of `numbers`, each asking `fields` of an Issue, because GraphQL has no way to look up a
/// list of numbers and a `$` variable can't add a field. Only digits reach this text.
pub(super) fn query_by_number(operation: &str, numbers: &[u32], fields: &str) -> String {
    let asked: Vec<String> = numbers
        .iter()
        .map(|number| {
            format!(
                "i{number}: issueOrPullRequest(number: {number}) {{ __typename ... on Issue {{ {fields} }} }}"
            )
        })
        .collect();
    format!(
        "query {operation}($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) \
         {{ {} }} rateLimit {{ remaining resetAt }} }}",
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

/// An answer to a read of Issues by number.
pub(super) struct ReadByNumber<T> {
    /// One for each number that names an Issue.
    pub issues: Vec<T>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

/// A PR that mentions an Issue. Only the link rules can say whether it belongs in the Issue's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedPr {
    pub id: NodeId,
    pub number: u32,
    #[serde(rename = "updatedAt", with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
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
         {REFERENCE_FIELDS} }} }} }} rateLimit {{ remaining resetAt }} }}"
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
            asked_for: read.asked_for,
            rate_limit: read.rate_limit,
        })
    }

    /// Asks for the first `read.size` of `read.numbers`, or half that after a 502 or no answer. A number can
    /// turn out to be a PR or to name nothing, and then the answer holds nothing for it.
    pub(super) fn read_by_number<T: DeserializeOwned>(
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
        let (data, asked_for): (Data<T>, usize) =
            self.query_at_size(size.min(numbers.len()), |size| {
                let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
                Request {
                    operation,
                    query: query(&asked).into(),
                    variables: json!({ "owner": repository.owner(), "name": repository.name() }),
                }
            })?;
        let issues = data
            .repository
            .into_values()
            .flatten()
            .filter_map(|numbered| match numbered {
                Numbered::Issue(issue) => Some(issue),
                Numbered::PullRequest => None,
            })
            .collect();
        Ok(ReadByNumber {
            issues,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
