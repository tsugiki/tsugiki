//! The pages of a first sync's window, each list newest first (ADR 0015).

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use time::OffsetDateTime;

use super::issues::{IssueFacts, IssueWire};
use super::pull_requests::{PULL_REQUEST_FIELDS, PullRequestFacts, PullRequestWire, RateLimit};
use super::refresh::{ListPlace, Listed, github_time};
use super::{GitHub, Request};
use crate::error::Error;
use crate::repository::RepositoryName;

const WINDOW_ISSUES: &str = "WindowIssues";
const WINDOW_PULL_REQUESTS: &str = "WindowPullRequests";

const WINDOW_ISSUES_QUERY: &str = "query WindowIssues($owner: String!, $name: String!, \
     $since: DateTime!, $first: Int!, $after: String) { repository(owner: $owner, name: $name) { \
     list: issues(first: $first, after: $after, filterBy: { since: $since }, \
     orderBy: { field: UPDATED_AT, direction: DESC }) { pageInfo { hasNextPage endCursor } \
     nodes { number title updatedAt author { login } } } } rateLimit { remaining resetAt } }";

/// GitHub's list of PRs takes no `since`, so the walk stops at the first PR older than the window.
fn window_pull_requests_query() -> String {
    format!(
        "query {WINDOW_PULL_REQUESTS}($owner: String!, $name: String!, $first: Int!, $after: String) {{ \
         repository(owner: $owner, name: $name) {{ list: pullRequests(first: $first, after: $after, \
         orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ pageInfo {{ hasNextPage endCursor }} \
         nodes {{ {PULL_REQUEST_FIELDS} }} }} }} rateLimit {{ remaining resetAt }} }}"
    )
}

/// One page of a list in the window.
#[derive(Debug, Clone, Copy)]
pub struct WindowRequest<'a> {
    /// Only what changed since this counts.
    pub since: OffsetDateTime,
    pub place: &'a ListPlace,
    pub size: usize,
}

/// One answer: the items on the page, oldest last.
#[derive(Debug)]
pub struct WindowPage<T> {
    pub items: Vec<T>,
    pub next: ListPlace,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
struct Holder<T> {
    list: Listed<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
struct Data<T> {
    repository: Holder<T>,
    rate_limit: RateLimit,
}

impl GitHub {
    /// One page of the Issues updated since `asked.since`. `asked.size` halves after a 502 or no answer, and
    /// the cursor carries on from there.
    pub fn window_issues(
        &self,
        repository: &RepositoryName,
        asked: WindowRequest<'_>,
    ) -> Result<WindowPage<IssueFacts>, Error> {
        let (data, _): (Data<IssueWire>, usize) =
            self.query_at_size(asked.size, |first| Request {
                operation: WINDOW_ISSUES,
                query: WINDOW_ISSUES_QUERY.into(),
                variables: json!({
                    "owner": repository.owner(),
                    "name": repository.name(),
                    "since": github_time(asked.since),
                    "first": first,
                    "after": asked.place.after(),
                }),
            })?;
        let next = asked.place.after_answer(data.repository.list.page_info);
        let nodes = data.repository.list.nodes.into_iter().flatten();
        Ok(WindowPage {
            items: nodes.map(IssueFacts::from).collect(),
            next,
            rate_limit: data.rate_limit,
        })
    }

    /// One page of the PRs updated since `asked.since`. `asked.size` halves after a 502 or no answer, and the
    /// cursor carries on from there. A PR older than `asked.since` ends the list, which runs newest first.
    pub fn window_pull_requests(
        &self,
        repository: &RepositoryName,
        asked: WindowRequest<'_>,
    ) -> Result<WindowPage<PullRequestFacts>, Error> {
        let query = window_pull_requests_query();
        let (data, _): (Data<PullRequestWire>, usize) =
            self.query_at_size(asked.size, |first| Request {
                operation: WINDOW_PULL_REQUESTS,
                query: query.as_str().into(),
                variables: json!({
                    "owner": repository.owner(),
                    "name": repository.name(),
                    "first": first,
                    "after": asked.place.after(),
                }),
            })?;
        let nodes = data.repository.list.nodes.into_iter().flatten();
        let mut items: Vec<PullRequestFacts> = nodes.map(PullRequestFacts::from).collect();
        let read = items.len();
        items.retain(|pr| pr.updated_at >= asked.since);
        let next = if items.len() < read {
            ListPlace::Finished
        } else {
            asked.place.after_answer(data.repository.list.page_info)
        };
        Ok(WindowPage {
            items,
            next,
            rate_limit: data.rate_limit,
        })
    }
}
