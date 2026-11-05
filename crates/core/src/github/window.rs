//! The pages of a first sync's window, each list newest first (ADR 0015).

use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use super::activity::ItemActivity;
use super::issues::{ISSUE_FIELDS, IssueFacts, IssueWire};
use super::pull_requests::RateLimit;
use super::references::{IssueReferences, REFERENCE_FIELDS, ReferenceTimeline};
use super::refresh::{ChangedPr, ListPlace, Listed, PageInfo, github_time};
use super::{GitHub, Request};
use crate::error::Error;
use crate::repository::RepositoryName;

const WINDOW_ISSUES: &str = "WindowIssues";
const WINDOW_PULL_REQUESTS: &str = "WindowPullRequests";

/// Each Issue brings its cross-references, so a tree's PRs are known with no query per Issue (ADR 0011).
fn window_issues_query() -> String {
    format!(
        "query {WINDOW_ISSUES}($owner: String!, $name: String!, $since: DateTime!, $first: Int!, \
         $after: String) {{ repository(owner: $owner, name: $name) {{ \
         list: issues(first: $first, after: $after, filterBy: {{ since: $since }}, \
         orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ totalCount pageInfo {{ hasNextPage endCursor }} \
         nodes {{ {ISSUE_FIELDS} references: timelineItems(last: 50, itemTypes: [CROSS_REFERENCED_EVENT]) \
         {{ {REFERENCE_FIELDS} }} }} }} }} rateLimit {{ remaining resetAt }} }}"
    )
}

/// GitHub's list of PRs takes no `since`, so the walk stops at the first PR older than the window. It asks
/// only for what says whether to fetch a PR, because a PR is fetched in full once, from the queue.
const WINDOW_PULL_REQUESTS_QUERY: &str = "query WindowPullRequests($owner: String!, $name: String!, $first: \
     Int!, $after: String) { repository(owner: $owner, name: $name) { list: pullRequests(first: $first, \
     after: $after, orderBy: { field: UPDATED_AT, direction: DESC }) { pageInfo { hasNextPage endCursor } \
     nodes { id number updatedAt } } } rateLimit { remaining resetAt } }";

/// One page of a list in the window.
#[derive(Debug, Clone, Copy)]
pub struct WindowRequest<'a> {
    /// Only what changed since this counts.
    pub since: OffsetDateTime,
    pub place: &'a ListPlace,
    pub size: usize,
}

/// A page of Issues, oldest last, with each one's cross-references.
#[derive(Debug)]
pub struct WindowIssues {
    pub items: Vec<IssueFacts>,
    pub activity: Vec<ItemActivity>,
    pub references: Vec<IssueReferences>,
    /// The count of Issues in the whole list, for the status bar.
    pub total: u32,
    pub next: ListPlace,
    pub rate_limit: RateLimit,
}

/// A page of PRs, oldest last, with no more than a fetch needs.
#[derive(Debug)]
pub struct WindowPullRequests {
    pub items: Vec<ChangedPr>,
    pub next: ListPlace,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
struct IssueWithReferences {
    #[serde(flatten)]
    issue: IssueWire,
    references: ReferenceTimeline,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueList {
    total_count: u32,
    page_info: PageInfo,
    nodes: Vec<Option<IssueWithReferences>>,
}

#[derive(Deserialize)]
struct IssueHolder {
    list: IssueList,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueData {
    repository: IssueHolder,
    rate_limit: RateLimit,
}

#[derive(Deserialize)]
struct PullRequestHolder {
    list: Listed<ChangedPr>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequestData {
    repository: PullRequestHolder,
    rate_limit: RateLimit,
}

impl GitHub {
    /// One page of the Issues updated since `asked.since`. `asked.size` halves after a 502 or no answer, and
    /// the cursor carries on from there.
    pub fn window_issues(
        &self,
        repository: &RepositoryName,
        asked: WindowRequest<'_>,
    ) -> Result<WindowIssues, Error> {
        let query = window_issues_query();
        let (data, _): (IssueData, usize) = self.query_at_size(asked.size, |first| Request {
            operation: WINDOW_ISSUES,
            query: query.as_str().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "since": github_time(asked.since),
                "first": first,
                "after": asked.place.after(),
            }),
        })?;
        let list = data.repository.list;
        let next = asked.place.after_answer(list.page_info);
        let mut references = Vec::new();
        let (items, activity): (Vec<IssueFacts>, Vec<ItemActivity>) = list
            .nodes
            .into_iter()
            .flatten()
            .map(|node| {
                let (facts, activity): (IssueFacts, ItemActivity) = node.issue.into();
                let (listed, next) = node.references.read(repository);
                references.push(IssueReferences {
                    number: facts.number,
                    listed,
                    next,
                });
                (facts, activity)
            })
            .unzip();
        Ok(WindowIssues {
            items,
            activity,
            references,
            total: list.total_count,
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
    ) -> Result<WindowPullRequests, Error> {
        let (data, _): (PullRequestData, usize) =
            self.query_at_size(asked.size, |first| Request {
                operation: WINDOW_PULL_REQUESTS,
                query: WINDOW_PULL_REQUESTS_QUERY.into(),
                variables: json!({
                    "owner": repository.owner(),
                    "name": repository.name(),
                    "first": first,
                    "after": asked.place.after(),
                }),
            })?;
        let list = data.repository.list;
        let mut items: Vec<ChangedPr> = list.nodes.into_iter().flatten().collect();
        let read = items.len();
        items.retain(|pr| pr.updated_at >= asked.since);
        let next = if items.len() < read {
            ListPlace::Finished
        } else {
            asked.place.after_answer(list.page_info)
        };
        Ok(WindowPullRequests {
            items,
            next,
            rate_limit: data.rate_limit,
        })
    }
}
