//! The Issues that PRs name, fetched by number, since GitHub has no search for a list of numbers. A number
//! can turn out to be a PR or to name nothing, and then Tsugiki keeps nothing for it (ADR 0011). An Issue's
//! cross-references list the PRs that may belong in its tree.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use super::pull_requests::{NodeId, PageRequest, Person, RateLimit};
use super::{GitHub, Login, Request};
use crate::error::Error;
use crate::repository::RepositoryName;

/// An Issue a PR names, as Tsugiki keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueFacts {
    pub number: u32,
    pub title: String,
    /// Nothing, for an Issue whose author's account has gone.
    pub author: Option<Login>,
    pub updated_at: OffsetDateTime,
}

/// One answer to a fetch by number.
#[derive(Debug)]
pub struct FetchedIssues {
    /// One for each number that names an Issue.
    pub issues: Vec<IssueFacts>,
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
}

impl From<IssueWire> for IssueFacts {
    fn from(wire: IssueWire) -> Self {
        Self {
            number: wire.number,
            title: wire.title,
            author: wire.author.map(|person| person.login),
            updated_at: wire.updated_at,
        }
    }
}

/// GitHub's answer for one number.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Numbered {
    Issue(IssueWire),
    PullRequest,
}

/// The query for `numbers`, with one field for each, because GraphQL has no way to look up a list of numbers
/// and a `$` variable can't add a field. Only digits reach this text.
fn fetch_issues_query(numbers: &[u32]) -> String {
    let fields: Vec<String> = numbers
        .iter()
        .map(|number| {
            format!(
                "i{number}: issueOrPullRequest(number: {number}) {{ __typename ... on Issue {{ number \
                 title updatedAt author {{ login }} }} }}"
            )
        })
        .collect();
    format!(
        "query FetchIssues($owner: String!, $name: String!) {{ repository(owner: $owner, name: $name) \
         {{ {} }} rateLimit {{ remaining resetAt }} }}",
        fields.join(" ")
    )
}

/// A PR that mentions an Issue. Only the link rules can say whether it belongs in the Issue's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedPr {
    pub id: NodeId,
    pub number: u32,
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
const FETCH_CROSS_REFERENCES: &str = "query FetchCrossReferences($owner: String!, $name: String!, $number: Int!, $last: Int!, $before: String) { \
     repository(owner: $owner, name: $name) { issue(number: $number) { \
     timelineItems(last: $last, before: $before, itemTypes: [CROSS_REFERENCED_EVENT]) { \
     pageInfo { hasPreviousPage startCursor } nodes { ... on CrossReferencedEvent { source { __typename \
     ... on PullRequest { id number repository { nameWithOwner } } } } } } } } \
     rateLimit { remaining resetAt } }";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedRepository {
    name_with_owner: RepositoryName,
}

#[derive(Deserialize)]
struct SourcePr {
    #[serde(flatten)]
    listed: ListedPr,
    repository: NamedRepository,
}

/// The Issue or PR that mentioned the Issue.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Source {
    PullRequest(SourcePr),
    Issue,
}

#[derive(Deserialize)]
struct CrossReferenceEvent {
    source: Source,
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
        struct PageInfo {
            has_previous_page: bool,
            start_cursor: Option<String>,
        }
        #[derive(Deserialize)]
        struct Timeline {
            #[serde(rename = "pageInfo")]
            page_info: PageInfo,
            nodes: Vec<Option<CrossReferenceEvent>>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Issue {
            timeline_items: Timeline,
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
            query: FETCH_CROSS_REFERENCES.into(),
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
        let timeline = issue.timeline_items;
        let info = timeline.page_info;
        let listed = timeline
            .nodes
            .into_iter()
            .rev()
            .flatten()
            .filter_map(|event| match event.source {
                Source::PullRequest(pr) if pr.repository.name_with_owner.is_same(repository) => {
                    Some(pr.listed)
                }
                Source::PullRequest(_) | Source::Issue => None,
            })
            .collect();
        Ok(CrossReferences {
            listed,
            next: info.start_cursor.filter(|_| info.has_previous_page),
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
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: BTreeMap<String, Option<Numbered>>,
            rate_limit: RateLimit,
        }
        let (data, asked_for): (Data, usize) =
            self.query_at_size(size.min(numbers.len()), |size| {
                let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
                Request {
                    operation: "FetchIssues",
                    query: fetch_issues_query(&asked).into(),
                    variables: json!({ "owner": repository.owner(), "name": repository.name() }),
                }
            })?;
        let issues = data
            .repository
            .into_values()
            .flatten()
            .filter_map(|numbered| match numbered {
                Numbered::Issue(wire) => Some(IssueFacts::from(wire)),
                Numbered::PullRequest => None,
            })
            .collect();
        Ok(FetchedIssues {
            issues,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
