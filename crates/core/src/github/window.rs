//! The pages of a first sync's lists (ADR 0015): the window, newest first, and a small Repository's older
//! open items, oldest first.

use serde::{Deserialize, Serialize};
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
const OLDER_OPEN_ISSUES: &str = "OlderOpenIssues";
const OLDER_OPEN_PULL_REQUESTS: &str = "OlderOpenPullRequests";

/// The part of a first sync's lists a page belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Span {
    /// The items that changed inside the window, newest first.
    Window,
    /// A small Repository's open items that last changed before the window, oldest first. Closing an item
    /// changes it, so the window already holds what closed inside it.
    OlderOpen,
}

impl Span {
    /// True for an item this list keeps. GitHub's list of PRs takes no `since`, and open items can't be asked
    /// for by a time they changed before, so each of those lists ends at the first item past `since`.
    fn keeps(self, updated_at: OffsetDateTime, since: OffsetDateTime) -> bool {
        match self {
            Self::Window => updated_at >= since,
            Self::OlderOpen => updated_at < since,
        }
    }
}

/// The operation and query of a list of Issues. Each Issue brings its cross-references, so a tree's PRs are
/// known with no query per Issue (ADR 0011).
fn issues_query(span: Span) -> (&'static str, String) {
    let (operation, since, list) = match span {
        Span::Window => (
            WINDOW_ISSUES,
            " $since: DateTime!,",
            "filterBy: { since: $since }, orderBy: { field: UPDATED_AT, direction: DESC }",
        ),
        Span::OlderOpen => (
            OLDER_OPEN_ISSUES,
            "",
            "states: OPEN, orderBy: { field: UPDATED_AT, direction: ASC }",
        ),
    };
    let query = format!(
        "query {operation}($owner: String!, $name: String!,{since} $first: Int!, \
         $after: String) {{ repository(owner: $owner, name: $name) {{ \
         list: issues(first: $first, after: $after, {list}) {{ totalCount pageInfo {{ hasNextPage endCursor }} \
         nodes {{ {ISSUE_FIELDS} references: timelineItems(last: 50, itemTypes: [CROSS_REFERENCED_EVENT]) \
         {{ {REFERENCE_FIELDS} }} }} }} }} rateLimit {{ remaining resetAt }} }}"
    );
    (operation, query)
}

/// The place of a list after a page that read `read` items and kept `kept`. A list that dropped one has
/// reached its end.
fn after_page(place: &ListPlace, read: usize, kept: usize, page: PageInfo) -> ListPlace {
    if kept < read {
        ListPlace::Finished
    } else {
        place.after_answer(page)
    }
}

/// GitHub's list of PRs takes no `since`, so the walk stops at the first PR older than the window. It asks
/// only for what says whether to fetch a PR, because a PR is fetched in full once, from the queue.
const WINDOW_PULL_REQUESTS_QUERY: &str = "query WindowPullRequests($owner: String!, $name: String!, $first: \
     Int!, $after: String) { repository(owner: $owner, name: $name) { list: pullRequests(first: $first, \
     after: $after, orderBy: { field: UPDATED_AT, direction: DESC }) { pageInfo { hasNextPage endCursor } \
     nodes { id number updatedAt } } } rateLimit { remaining resetAt } }";

const OLDER_OPEN_PULL_REQUESTS_QUERY: &str = "query OlderOpenPullRequests($owner: String!, $name: String!, \
     $first: Int!, $after: String) { repository(owner: $owner, name: $name) { list: pullRequests(first: $first, \
     after: $after, states: OPEN, orderBy: { field: UPDATED_AT, direction: ASC }) { pageInfo { hasNextPage \
     endCursor } nodes { id number updatedAt } } } rateLimit { remaining resetAt } }";

/// One page of one of a first sync's lists.
#[derive(Debug, Clone, Copy)]
pub struct ListRequest<'a> {
    pub span: Span,
    /// The start of the window.
    pub since: OffsetDateTime,
    pub place: &'a ListPlace,
    pub size: usize,
}

/// A page of Issues in the order of its list, with each one's cross-references.
#[derive(Debug)]
pub struct ListedIssues {
    pub items: Vec<IssueFacts>,
    pub activity: Vec<ItemActivity>,
    pub references: Vec<IssueReferences>,
    /// The count of Issues in the whole window, for the status bar. GitHub can't count the older ones.
    pub total: Option<u32>,
    pub next: ListPlace,
    pub rate_limit: RateLimit,
}

/// A page of PRs in the order of its list, with no more than a fetch needs.
#[derive(Debug)]
pub struct ListedPullRequests {
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
    /// One page of a first sync's list of Issues. `asked.size` halves after a 502 or no answer, and the
    /// cursor carries on from there.
    pub fn list_issues(
        &self,
        repository: &RepositoryName,
        asked: ListRequest<'_>,
    ) -> Result<ListedIssues, Error> {
        let (operation, query) = issues_query(asked.span);
        let (data, _): (IssueData, usize) = self.query_at_size(asked.size, |first| {
            let (owner, name, after) = (repository.owner(), repository.name(), asked.place.after());
            let variables = match asked.span {
                Span::Window => json!({
                    "owner": owner,
                    "name": name,
                    "since": github_time(asked.since),
                    "first": first,
                    "after": after,
                }),
                Span::OlderOpen => {
                    json!({ "owner": owner, "name": name, "first": first, "after": after })
                }
            };
            Request {
                operation,
                query: query.as_str().into(),
                variables,
            }
        })?;
        let list = data.repository.list;
        let nodes: Vec<IssueWithReferences> = list.nodes.into_iter().flatten().collect();
        let read = nodes.len();
        let mut references = Vec::new();
        let (items, activity): (Vec<IssueFacts>, Vec<ItemActivity>) = nodes
            .into_iter()
            .map(|node| {
                let (facts, activity): (IssueFacts, ItemActivity) = node.issue.into();
                (facts, activity, node.references)
            })
            .filter(|(facts, _, _)| asked.span.keeps(facts.updated_at, asked.since))
            .map(|(facts, activity, timeline)| {
                let (listed, next) = timeline.read(repository);
                references.push(IssueReferences {
                    number: facts.number,
                    listed,
                    next,
                });
                (facts, activity)
            })
            .unzip();
        let next = after_page(asked.place, read, items.len(), list.page_info);
        Ok(ListedIssues {
            items,
            activity,
            references,
            total: (asked.span == Span::Window).then_some(list.total_count),
            next,
            rate_limit: data.rate_limit,
        })
    }

    /// One page of a first sync's list of PRs. `asked.size` halves after a 502 or no answer, and the cursor
    /// carries on from there.
    pub fn list_pull_requests(
        &self,
        repository: &RepositoryName,
        asked: ListRequest<'_>,
    ) -> Result<ListedPullRequests, Error> {
        let (operation, query) = match asked.span {
            Span::Window => (WINDOW_PULL_REQUESTS, WINDOW_PULL_REQUESTS_QUERY),
            Span::OlderOpen => (OLDER_OPEN_PULL_REQUESTS, OLDER_OPEN_PULL_REQUESTS_QUERY),
        };
        let (data, _): (PullRequestData, usize) =
            self.query_at_size(asked.size, |first| Request {
                operation,
                query: query.into(),
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
        items.retain(|pr| asked.span.keeps(pr.updated_at, asked.since));
        let next = after_page(asked.place, read, items.len(), list.page_info);
        Ok(ListedPullRequests {
            items,
            next,
            rate_limit: data.rate_limit,
        })
    }
}
