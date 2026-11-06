//! A refresh's questions (ADR 0015). One REST request says whether anything changed, one GraphQL request
//! lists what did, and a probe reads what changes without moving `updatedAt`. Together they cost a few
//! points.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::activity::{ItemActivity, TimelineWire, pull_request_activity};
use super::filters::{FOUND_FIELDS, FilterPage, FilterSearch, SearchWire};
use super::issues::{ISSUE_FIELDS, IssueFacts, IssueWire};
use super::pull_requests::{
    CiState, LastCommitNode, LatestReview, LatestReviewWire, NodeId, Nodes, PageRequest,
    PullRequestFacts, RateLimit, ReviewRequestWire, ci_of, requested_logins, reviews_of,
};
use super::{ConditionalGet, GitHub, Login, Request};
use crate::conventions::Conventions;
use crate::error::Error;
use crate::filters::FilterQuery;
use crate::repository::RepositoryName;

const REFRESH_REPOSITORY: &str = "RefreshRepository";
const PROBE_PULL_REQUESTS: &str = "ProbePullRequests";
const NOT_MODIFIED: u16 = 304;
const OK: u16 = 200;

/// GitHub's tag for an answer: asking with it answers 304, for free, while the answer stays the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Etag(String);

impl Etag {
    pub fn new(tag: &str) -> Self {
        Self(tag.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What GitHub said about a Repository's list of Issues and PRs.
#[derive(Debug, PartialEq, Eq)]
pub enum ListCheck {
    /// Nothing in the list moved since the tag.
    Same,
    /// Something moved, or GitHub didn't say, so a refresh has to ask. This holds the list's tag now, if
    /// GitHub gave one.
    Moved(Option<Etag>),
}

/// How far a poll has read one of its two lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListPlace {
    /// The next page to read, from this cursor. The first page has none.
    Next(Option<String>),
    Finished,
}

impl ListPlace {
    pub fn is_finished(&self) -> bool {
        *self == Self::Finished
    }

    pub(super) fn after(&self) -> Option<&str> {
        match self {
            Self::Next(cursor) => cursor.as_deref(),
            Self::Finished => None,
        }
    }

    /// The count to ask for. GitHub answers a count of 0 with no items, for a list already read.
    fn first(&self, size: usize) -> usize {
        match self {
            Self::Next(_) => size,
            Self::Finished => 0,
        }
    }

    /// Where this list stands after an answer that had this `page`.
    pub(super) fn after_answer(&self, page: PageInfo) -> Self {
        match (self, page.end_cursor.filter(|_| page.has_next_page)) {
            (Self::Next(_), Some(cursor)) => Self::Next(Some(cursor)),
            (Self::Next(_) | Self::Finished, None) | (Self::Finished, Some(_)) => Self::Finished,
        }
    }
}

/// One request of a poll.
#[derive(Debug, Clone, Copy)]
pub struct PollRequest<'a> {
    /// Only what changed since this counts.
    pub since: OffsetDateTime,
    pub issues: &'a ListPlace,
    pub pull_requests: &'a ListPlace,
    /// The searches to narrow to `since` and run in this same request.
    pub filters: &'a [FilterQuery],
    pub size: usize,
}

/// A PR that changed since a poll's `since`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedPr {
    pub id: NodeId,
    pub number: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// One answer to a poll.
#[derive(Debug, PartialEq, Eq)]
pub struct PollAnswer {
    /// The open Issues and PRs, which say whether this Repository counts as small.
    pub open_items: u32,
    pub issues: Vec<IssueFacts>,
    pub issue_activity: Vec<ItemActivity>,
    pub next_issues: ListPlace,
    pub pull_requests: Vec<ChangedPr>,
    pub next_pull_requests: ListPlace,
    /// One first page for each search asked for.
    pub filters: Vec<(FilterQuery, FilterPage)>,
    pub rate_limit: RateLimit,
}

/// What a probe of a PR read: the facts that can change while `updatedAt` stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrProbe {
    pub number: u32,
    pub ci: Option<CiState>,
    pub review_requests: Vec<Login>,
    pub latest_reviews: Vec<LatestReview>,
}

impl PrProbe {
    /// `stored`, with what this probe read.
    pub fn applied_to(self, stored: &PullRequestFacts) -> PullRequestFacts {
        PullRequestFacts {
            ci: self.ci,
            review_requests: self.review_requests,
            latest_reviews: self.latest_reviews,
            ..stored.clone()
        }
    }
}

/// One answer to a probe.
#[derive(Debug, PartialEq, Eq)]
pub struct ProbedBatch {
    /// One for each number that still names a PR.
    pub probes: Vec<PrProbe>,
    pub activity: Vec<ItemActivity>,
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

/// A page of a list GitHub can leave items out of.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Listed<T> {
    pub(super) page_info: PageInfo,
    pub(super) nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeWire {
    number: u32,
    review_requests: Nodes<ReviewRequestWire>,
    latest_reviews: Nodes<LatestReviewWire>,
    last_commit: Nodes<LastCommitNode>,
    timeline_items: Nodes<TimelineWire>,
}

impl ProbeWire {
    /// The probe, and the Reviews its timeline lists. These Reviews have no comments for Conventions to read.
    fn into_parts(self) -> (PrProbe, ItemActivity) {
        let activity = pull_request_activity(self.number, self.timeline_items, Conventions::Github);
        let probe = PrProbe {
            number: self.number,
            ci: ci_of(self.last_commit),
            review_requests: requested_logins(self.review_requests),
            latest_reviews: reviews_of(self.latest_reviews),
        };
        (probe, activity)
    }
}

/// The text of a poll with `searches` narrowed Filters, because a `$` variable can't add a field. GitHub
/// refuses a query that declares a variable it doesn't use, so `$first` comes only with a search.
fn poll_query(searches: usize) -> String {
    let filters = (0..searches).map(|at| format!(", $filter{at}: String!"));
    let declared: String = (searches > 0)
        .then(|| ", $first: Int!".to_owned())
        .into_iter()
        .chain(filters)
        .collect();
    let asked: String = (0..searches)
        .map(|at| {
            format!(
                "f{at}: search(query: $filter{at}, type: ISSUE_ADVANCED, first: $first) {{ issueCount \
                 pageInfo {{ hasNextPage endCursor }} nodes {{ {FOUND_FIELDS} }} }} "
            )
        })
        .collect();
    format!(
        "query {REFRESH_REPOSITORY}($owner: String!, $name: String!, $since: DateTime!, \
         $issues: Int!, $issuesAfter: String, $prs: Int!, $prsAfter: String{declared}) {{ \
         repository(owner: $owner, name: $name) {{ \
         openIssues: issues(states: OPEN) {{ totalCount }} \
         openPrs: pullRequests(states: OPEN) {{ totalCount }} \
         issues(first: $issues, after: $issuesAfter, filterBy: {{ since: $since }}, \
         orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ pageInfo {{ hasNextPage endCursor }} \
         nodes {{ {ISSUE_FIELDS} }} }} \
         pullRequests(first: $prs, after: $prsAfter, orderBy: {{ field: UPDATED_AT, direction: DESC }}) {{ \
         pageInfo {{ hasNextPage endCursor }} nodes {{ id number updatedAt }} }} }} \
         {asked}rateLimit {{ remaining resetAt }} }}"
    )
}

/// The text of a probe of `numbers`, with one field for each. Only digits reach this text.
fn probe_query(numbers: &[u32]) -> String {
    let fields: Vec<String> = numbers
        .iter()
        .map(|number| {
            format!(
                "p{number}: pullRequest(number: {number}) {{ number \
                 reviewRequests(first: 100) {{ nodes {{ requestedReviewer {{ \
                 ... on User {{ login }} ... on Bot {{ login }} }} }} }} \
                 latestReviews(first: 100) {{ nodes {{ author {{ login }} state }} }} \
                 lastCommit: commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} \
                 timelineItems(last: 20, itemTypes: [PULL_REQUEST_REVIEW]) {{ nodes {{ __typename \
                 ... on PullRequestReview {{ submittedAt author {{ __typename login }} }} }} }} }}"
            )
        })
        .collect();
    format!(
        "query {PROBE_PULL_REQUESTS}($owner: String!, $name: String!) {{ \
         repository(owner: $owner, name: $name) {{ {} }} rateLimit {{ remaining resetAt }} }}",
        fields.join(" ")
    )
}

/// `at` as GitHub reads it. Only a year past 9999 can't be written this way.
pub(super) fn github_time(at: OffsetDateTime) -> String {
    at.format(&Rfc3339).unwrap_or_default()
}

impl GitHub {
    /// Asks whether anything in `repository` moved since `tag`. A 304 costs nothing. After a failure of any
    /// kind the refresh asks by GraphQL, which reports what went wrong.
    pub fn check_list(&self, repository: &RepositoryName, tag: Option<&Etag>) -> ListCheck {
        let path = format!(
            "/repos/{}/{}/issues?state=all&sort=updated&per_page=1",
            repository.owner(),
            repository.name()
        );
        let request = ConditionalGet {
            path: &path,
            etag: tag.map(Etag::as_str),
        };
        let reply = self.transport.get(&self.token, &request);
        if let Ok(reply) = &reply
            && (reply.status == NOT_MODIFIED || reply.status == OK)
        {
            self.set_clock(reply.date);
        }
        match reply {
            Ok(reply) if reply.status == NOT_MODIFIED => ListCheck::Same,
            Ok(reply) if reply.status == OK => {
                ListCheck::Moved(reply.etag.as_deref().map(Etag::new))
            }
            Ok(reply) => {
                log::warn!("GitHub answered a list check with status {}", reply.status);
                ListCheck::Moved(None)
            }
            Err(_) => ListCheck::Moved(None),
        }
    }

    /// One request of a poll. `asked.size` halves after a 502 or no answer, and the cursors carry on from
    /// there. A PR older than `asked.since` ends its list, which runs newest first.
    pub fn poll_repository(
        &self,
        repository: &RepositoryName,
        asked: &PollRequest<'_>,
    ) -> Result<PollAnswer, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Total {
            total_count: u32,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Polled {
            open_issues: Total,
            open_prs: Total,
            issues: Listed<IssueWire>,
            pull_requests: Listed<ChangedPr>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Polled,
            rate_limit: RateLimit,
            #[serde(flatten)]
            searches: BTreeMap<String, SearchWire>,
        }
        let query = poll_query(asked.filters.len());
        let (data, _): (Data, usize) = self.query_at_size(asked.size, |first| Request {
            operation: REFRESH_REPOSITORY,
            query: query.as_str().into(),
            variables: poll_variables(repository, asked, first),
        })?;
        let Data {
            repository: polled,
            rate_limit,
            mut searches,
        } = data;
        let mut pull_requests: Vec<ChangedPr> =
            polled.pull_requests.nodes.into_iter().flatten().collect();
        let read = pull_requests.len();
        pull_requests.retain(|pr| pr.updated_at >= asked.since);
        let next_pull_requests = if pull_requests.len() < read {
            ListPlace::Finished
        } else {
            asked
                .pull_requests
                .after_answer(polled.pull_requests.page_info)
        };
        let answered = |(at, query): (usize, &FilterQuery)| {
            let wire = searches.remove(&format!("f{at}"))?;
            Some((query.clone(), wire.into_page(repository, rate_limit)))
        };
        let next_issues = asked.issues.after_answer(polled.issues.page_info);
        let polled_issues = polled.issues.nodes.into_iter().flatten();
        let (issues, issue_activity) = polled_issues.map(Into::into).unzip();
        Ok(PollAnswer {
            open_items: polled.open_issues.total_count + polled.open_prs.total_count,
            issues,
            issue_activity,
            next_issues,
            pull_requests,
            next_pull_requests,
            filters: asked
                .filters
                .iter()
                .enumerate()
                .filter_map(answered)
                .collect(),
            rate_limit,
        })
    }

    /// The first `size` of `numbers`, or half that after a 502 or no answer. `numbers` holds at least one.
    pub fn probe_pull_requests(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<ProbedBatch, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: BTreeMap<String, Option<ProbeWire>>,
            rate_limit: RateLimit,
        }
        let request = |size: usize| {
            let asked: Vec<u32> = numbers.iter().take(size).copied().collect();
            Request {
                operation: PROBE_PULL_REQUESTS,
                query: probe_query(&asked).into(),
                variables: json!({ "owner": repository.owner(), "name": repository.name() }),
            }
        };
        let (data, asked_for): (Data, usize) =
            self.query_at_size(size.min(numbers.len()), request)?;
        let answers = data.repository.into_values().flatten();
        let (probes, activity) = answers.map(ProbeWire::into_parts).unzip();
        Ok(ProbedBatch {
            probes,
            activity,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }
}

fn poll_variables(repository: &RepositoryName, asked: &PollRequest<'_>, first: usize) -> Value {
    let named = [
        ("owner", json!(repository.owner())),
        ("name", json!(repository.name())),
        ("since", json!(github_time(asked.since))),
        ("first", json!(first)),
        ("issues", json!(asked.issues.first(first))),
        ("issuesAfter", json!(asked.issues.after())),
        ("prs", json!(asked.pull_requests.first(first))),
        ("prsAfter", json!(asked.pull_requests.after())),
    ];
    let mut variables: Map<String, Value> = named
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    for (at, query) in asked.filters.iter().enumerate() {
        let search = FilterSearch {
            query,
            created: None,
            since: Some(asked.since),
            page: PageRequest {
                size: first,
                after: None,
            },
        };
        let text = Value::from(search.text(repository));
        variables.extend([(format!("filter{at}"), text)]);
    }
    Value::Object(variables)
}

#[cfg(test)]
mod tests;
