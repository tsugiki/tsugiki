//! Finding and fetching PRs on GitHub, and what comes back in the form the rest of Tsugiki reads. What to
//! look for, how much to ask for and when to pause are Tsugiki's choices, and `sync` makes them
//! (ADR 0015).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use super::activity::{TimelineWire, pull_request_activity};
use super::labels::{LabelWire, label_names};
use super::{BAD_GATEWAY, GitHub, RATE_LIMIT_FIELDS, Request, Unreachable};
use crate::conventions::Conventions;
use crate::error::Error;
use crate::facts::CommitSha;
use crate::facts::activity::ItemActivity;
use crate::facts::pull_requests::{
    CiState, LatestReview, NodeId, OffDefault, PrState, PullRequestFacts, ReviewState,
};
use crate::login::Login;
use crate::repository::RepositoryName;

/// One search for PRs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrSearch {
    /// Names the request, so each search has its own recording.
    pub operation: &'static str,
    /// GitHub's search words, without the Repository.
    pub qualifiers: &'static str,
}

/// The page of a search to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest<'a> {
    /// The number of IDs to ask for.
    pub size: usize,
    /// The cursor the last page gave for the next one.
    pub after: Option<&'a str>,
}

/// The text of a search for PR IDs, by GitHub's advanced search.
fn search_query(operation: &str) -> String {
    format!(
        "query {operation}($query: String!, $first: Int!, $after: String) {{ \
         search(query: $query, type: ISSUE_ADVANCED, first: $first, after: $after) {{ \
         pageInfo {{ hasNextPage endCursor }} nodes {{ ... on PullRequest {{ id }} }} }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

/// The Workbench works out from these lists why a PR is the user's, so a list cut short would hide a PR the
/// searches found. GitHub returns at most 100 at a time, and it limits a PR to 10 assignees. `lastCommit`
/// asks for CI on its own, because CI rollups are slow and `commits` can run to 100. The last 50 items of a
/// timeline are enough for New activity, which has to say that someone did something lately.
pub(super) const PULL_REQUEST_FIELDS: &str = "number title state isDraft updatedAt baseRefName body \
                                   mergeCommit { oid } baseRefOid headRefOid \
                                   repository { nameWithOwner defaultBranchRef { name } } \
                                   closingIssuesReferences(first: 100) { nodes { number \
                                   repository { nameWithOwner } } } author { login } assignees(first: 20) { nodes { login } } \
                                   labels(first: 100) { nodes { name } } \
                                   reviewRequests(first: 100) { nodes { requestedReviewer { \
                                   ... on User { login } ... on Bot { login } } } } \
                                   latestReviews(first: 100) { nodes { author { login } state } } \
                                   lastCommit: commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } \
                                   commits(first: 100) { totalCount nodes { commit { message \
                                   parents { totalCount } committer { user { login } } } } } \
                                   timelineItems(last: 50, itemTypes: [PULL_REQUEST_COMMIT, ISSUE_COMMENT, \
                                   PULL_REQUEST_REVIEW, READY_FOR_REVIEW_EVENT, HEAD_REF_FORCE_PUSHED_EVENT, \
                                   MENTIONED_EVENT, REVIEW_REQUESTED_EVENT, ASSIGNED_EVENT]) { \
                                   nodes { __typename \
                                   ... on PullRequestCommit { commit { committedDate \
                                   author { name user { login } } committer { name user { login } } } } \
                                   ... on IssueComment { createdAt author { __typename login } bodyText } \
                                   ... on PullRequestReview { submittedAt author { __typename login } } \
                                   ... on ReadyForReviewEvent { createdAt actor { __typename login } } \
                                   ... on HeadRefForcePushedEvent { createdAt actor { __typename login } } \
                                   ... on MentionedEvent { createdAt actor { __typename login } } \
                                   ... on ReviewRequestedEvent { createdAt requestedReviewer { \
                                   ... on User { login } ... on Bot { login } } } \
                                   ... on AssignedEvent { createdAt assignee { \
                                   ... on User { login } ... on Bot { login } } } } }";

/// The text of a fetch of PRs by ID.
fn fetch_by_ids_query(operation: PrFetch) -> String {
    format!(
        "query {}($ids: [ID!]!) {{ nodes(ids: $ids) {{ ... on PullRequest {{ {PULL_REQUEST_FIELDS} }} }} \
         {RATE_LIMIT_FIELDS} }}",
        operation.name()
    )
}

/// The text of a fetch of one PR by its number.
fn fetch_by_number_query() -> String {
    format!(
        "query {FETCH_ORIGIN}($owner: String!, $name: String!, $number: Int!) {{ \
         repository(owner: $owner, name: $name) {{ pullRequest(number: $number) {{ {PULL_REQUEST_FIELDS} }} }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

/// Names the fetch of a Backport's Origin PR by number, and so its recording.
const FETCH_ORIGIN: &str = "FetchOriginPullRequest";

/// The reason for a fetch by ID. Each has its own recording, because a recording holds one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrFetch {
    /// The PRs the user's searches found.
    Work,
    /// The PRs an Issue's cross-references list.
    Tree,
    /// The PRs a refresh found changed.
    Changed,
}

impl PrFetch {
    fn name(self) -> &'static str {
        match self {
            Self::Work => "FetchPullRequests",
            Self::Tree => "FetchTreePullRequests",
            Self::Changed => "FetchChangedPullRequests",
        }
    }
}

/// GitHub's count of the hour's points, as an answer reports it (ADR 0015).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    /// The points this query cost.
    pub cost: u32,
    /// The points an hour gives.
    pub limit: u32,
    /// The points spent this hour by every tool that uses the token.
    pub used: u32,
    pub remaining: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub reset_at: OffsetDateTime,
}

/// One page of a search.
#[derive(Debug, PartialEq, Eq)]
pub struct SearchPage {
    pub ids: Vec<NodeId>,
    /// The next page's cursor, if there is one.
    pub next: Option<String>,
    pub rate_limit: RateLimit,
}

/// One batch of fetched PRs.
#[derive(Debug, PartialEq, Eq)]
pub struct FetchedBatch {
    pub pull_requests: Vec<PullRequestFacts>,
    pub activity: Vec<ItemActivity>,
    /// The number of IDs it asked for, which is fewer than offered after a retry.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

/// One Origin PR fetched by number.
#[derive(Debug, PartialEq, Eq)]
pub struct FetchedOrigin {
    /// Nothing, when that number names an Issue or nothing.
    pub pull_request: Option<(PullRequestFacts, ItemActivity)>,
    pub rate_limit: RateLimit,
}

/// The SHAs named in each `(cherry picked from commit <sha>)` line of `text`.
fn cherry_picks(text: &str) -> impl Iterator<Item = CommitSha> + '_ {
    text.lines().filter_map(|line| {
        let sha = line
            .trim()
            .strip_prefix("(cherry picked from commit ")?
            .strip_suffix(')')?;
        CommitSha::parse(sha)
    })
}

#[derive(Deserialize)]
pub(super) struct Nodes<T> {
    /// GitHub's schema allows a null for anything in a list, such as an item the token can't see.
    pub(super) nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
pub(super) struct Person {
    pub(super) login: Login,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReviewRequestWire {
    requested_reviewer: Option<ReviewerWire>,
}

/// A team has no `login`, and a query that asks for none gets an empty object for it.
#[derive(Deserialize)]
pub(super) struct ReviewerWire {
    pub(super) login: Option<Login>,
}

#[derive(Deserialize)]
pub(super) struct LatestReviewWire {
    author: Option<Person>,
    state: ReviewState,
}

#[derive(Deserialize)]
pub(super) struct LastCommitNode {
    commit: LastCommitWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LastCommitWire {
    status_check_rollup: Option<RollupWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitsWire {
    total_count: usize,
    nodes: Vec<Option<CommitNode>>,
}

#[derive(Deserialize)]
struct CommitNode {
    commit: CommitWire,
}

#[derive(Deserialize)]
struct CommitWire {
    message: String,
    parents: CountWire,
    committer: Option<GitActorWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountWire {
    total_count: usize,
}

/// `user` is nothing for a committer with no GitHub account, as for the merge GitHub's Update branch makes.
#[derive(Deserialize)]
struct GitActorWire {
    user: Option<Person>,
}

#[derive(Deserialize)]
struct MergeCommitWire {
    oid: String,
}

#[derive(Deserialize)]
struct RollupWire {
    state: CiState,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepositoryWire {
    name_with_owner: RepositoryName,
    /// An empty Repository has no default branch, and then it has no PRs either.
    default_branch_ref: Option<BranchWire>,
}

#[derive(Deserialize)]
struct BranchWire {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClosingReferenceRepository {
    name_with_owner: RepositoryName,
}

#[derive(Deserialize)]
struct ClosingReference {
    number: u32,
    repository: ClosingReferenceRepository,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PullRequestWire {
    number: u32,
    title: String,
    state: PrState,
    is_draft: bool,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
    base_ref_name: String,
    body: String,
    merge_commit: Option<MergeCommitWire>,
    base_ref_oid: String,
    head_ref_oid: String,
    repository: RepositoryWire,
    closing_issues_references: Nodes<ClosingReference>,
    author: Option<Person>,
    assignees: Nodes<Person>,
    labels: Option<Nodes<LabelWire>>,
    review_requests: Nodes<ReviewRequestWire>,
    latest_reviews: Nodes<LatestReviewWire>,
    last_commit: Nodes<LastCommitNode>,
    commits: CommitsWire,
    timeline_items: Nodes<TimelineWire>,
}

impl CommitsWire {
    /// The facts of a PR off the default branch with this `description` and these commits.
    fn off_default(self, description: String) -> OffDefault {
        let commits: Vec<CommitWire> = self
            .nodes
            .into_iter()
            .flatten()
            .map(|node| node.commit)
            .collect();
        let unread = self.total_count.saturating_sub(commits.len());
        let mut cherry_picked_from: Vec<CommitSha> = cherry_picks(&description).collect();
        let named = commits
            .iter()
            .flat_map(|commit| cherry_picks(&commit.message));
        for sha in named {
            if !cherry_picked_from.contains(&sha) {
                cherry_picked_from.push(sha);
            }
        }
        let mut committers: Vec<Option<Login>> = commits
            .into_iter()
            .filter(|commit| commit.parents.total_count < 2)
            .map(|commit| Some(commit.committer?.user?.login))
            .collect();
        // A commit Tsugiki didn't read could be anyone's.
        committers.extend((unread > 0).then_some(None));
        OffDefault {
            description,
            cherry_picked_from,
            committers,
        }
    }
}

/// The people a PR asks for a Review. A team has no login, and the Workbench goes by the user's own.
pub(super) fn requested_logins(requests: Nodes<ReviewRequestWire>) -> Vec<Login> {
    let requests = requests.nodes.into_iter().flatten();
    let mut logins: Vec<Login> = requests
        .filter_map(|request| request.requested_reviewer?.login)
        .collect();
    logins.sort_by_cached_key(store_order);
    logins
}

/// The order the store reads logins back in, so facts from GitHub and from the store compare equal.
fn store_order(login: &Login) -> String {
    login.as_str().to_ascii_lowercase()
}

pub(super) fn reviews_of(reviews: Nodes<LatestReviewWire>) -> Vec<LatestReview> {
    let reviews = reviews.nodes.into_iter().flatten();
    let mut latest: Vec<LatestReview> = reviews
        .filter_map(|review| {
            let author = review.author?.login;
            Some(LatestReview {
                author,
                state: review.state,
            })
        })
        .collect();
    latest.sort_by_cached_key(|review| store_order(&review.author));
    latest
}

pub(super) fn ci_of(last_commit: Nodes<LastCommitNode>) -> Option<CiState> {
    let mut commits = last_commit.nodes.into_iter().flatten();
    let rollup = commits.next()?.commit.status_check_rollup?;
    Some(rollup.state)
}

impl From<PullRequestWire> for (PullRequestFacts, ItemActivity) {
    fn from(wire: PullRequestWire) -> Self {
        let conventions = Conventions::for_repository(&wire.repository.name_with_owner);
        let activity = pull_request_activity(wire.number, wire.timeline_items, conventions);
        let review_requests = requested_logins(wire.review_requests);
        let latest_reviews = reviews_of(wire.latest_reviews);
        let ci = ci_of(wire.last_commit);
        let mut assignees: Vec<Login> = wire
            .assignees
            .nodes
            .into_iter()
            .flatten()
            .map(|person| person.login)
            .collect();
        assignees.sort_by_cached_key(store_order);
        let here = wire.repository.name_with_owner;
        // A reference to another Repository's Issue can't put this PR under an Issue here (ADR 0011).
        let mut closing_issues: Vec<u32> = wire
            .closing_issues_references
            .nodes
            .into_iter()
            .flatten()
            .filter(|reference| reference.repository.name_with_owner.is_same(&here))
            .map(|reference| reference.number)
            .collect();
        closing_issues.sort_unstable();
        let on_default = wire
            .repository
            .default_branch_ref
            .is_none_or(|branch| branch.name == wire.base_ref_name);
        let facts = PullRequestFacts {
            number: wire.number,
            title: wire.title,
            state: wire.state,
            is_draft: wire.is_draft,
            author: wire.author.map(|person| person.login),
            target_branch: wire.base_ref_name,
            ci,
            updated_at: wire.updated_at,
            review_requests,
            latest_reviews,
            assignees,
            labels: label_names(wire.labels),
            closing_issues,
            merge_commit: wire
                .merge_commit
                .filter(|_| wire.state == PrState::Merged)
                .and_then(|commit| CommitSha::parse(&commit.oid)),
            base: CommitSha::parse(&wire.base_ref_oid),
            head: CommitSha::parse(&wire.head_ref_oid),
            off_default: (!on_default).then(|| wire.commits.off_default(wire.body)),
        };
        (facts, activity)
    }
}

impl GitHub {
    /// One page of `search` in `repository`, or half that page after a 502 or no answer.
    pub fn search_pull_requests(
        &self,
        search: &PrSearch,
        repository: &RepositoryName,
        page: PageRequest<'_>,
    ) -> Result<SearchPage, Error> {
        #[derive(Deserialize)]
        struct Found {
            id: NodeId,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct PageInfo {
            has_next_page: bool,
            end_cursor: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Search {
            page_info: PageInfo,
            nodes: Vec<Option<Found>>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            search: Search,
            rate_limit: RateLimit,
        }
        let text = format!("repo:{repository} {}", search.qualifiers);
        let query = search_query(search.operation);
        let (data, _): (Data, usize) = self.query_at_size(page.size, |first| Request {
            operation: search.operation,
            query: query.as_str().into(),
            variables: json!({ "query": text, "first": first, "after": page.after }),
        })?;
        let info = data.search.page_info;
        Ok(SearchPage {
            ids: data
                .search
                .nodes
                .into_iter()
                .flatten()
                .map(|found| found.id)
                .collect(),
            next: info.end_cursor.filter(|_| info.has_next_page),
            rate_limit: data.rate_limit,
        })
    }

    /// The first `size` of `ids`, or half that after a 502 or no answer. The answer leaves out an ID GitHub
    /// no longer knows.
    pub fn fetch_pull_requests(
        &self,
        why: PrFetch,
        ids: &[NodeId],
        size: usize,
    ) -> Result<FetchedBatch, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            nodes: Vec<Option<PullRequestWire>>,
            rate_limit: RateLimit,
        }
        let query = fetch_by_ids_query(why);
        let (data, asked_for): (Data, usize) = self.query_at_size(size.min(ids.len()), |size| {
            let asked: Vec<&str> = ids.iter().take(size).map(NodeId::as_str).collect();
            Request {
                operation: why.name(),
                query: query.as_str().into(),
                variables: json!({ "ids": asked }),
            }
        })?;
        let (pull_requests, activity) = data.nodes.into_iter().flatten().map(Into::into).unzip();
        Ok(FetchedBatch {
            pull_requests,
            activity,
            asked_for,
            rate_limit: data.rate_limit,
        })
    }

    /// The PR with this `number`, if `repository` has one: a Backport's Origin PR can be any number, so one
    /// request asks for one PR (ADR 0011).
    pub fn fetch_origin_pull_request(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<FetchedOrigin, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Holder {
            pull_request: Option<PullRequestWire>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            repository: Holder,
            rate_limit: RateLimit,
        }
        let data: Data = self.query(&Request {
            operation: FETCH_ORIGIN,
            query: fetch_by_number_query().into(),
            variables: json!({
                "owner": repository.owner(),
                "name": repository.name(),
                "number": number,
            }),
        })?;
        Ok(FetchedOrigin {
            pull_request: data.repository.pull_request.map(Into::into),
            rate_limit: data.rate_limit,
        })
    }

    /// Asks for `size` items, and after a 502 or no answer asks once more for half as many (ADR 0015). Says
    /// how many the answer is for. A certificate outside its dates would fail the smaller request too.
    pub(super) fn query_at_size<'a, D: DeserializeOwned>(
        &self,
        size: usize,
        request: impl Fn(usize) -> Request<'a>,
    ) -> Result<(D, usize), Error> {
        match self.post(&request(size)) {
            Ok(reply) if reply.status != BAD_GATEWAY => Ok((self.answer(&reply)?, size)),
            Err(cause @ Unreachable::CertificateDates) => Err(cause.into()),
            Ok(_) | Err(Unreachable::Offline) => {
                let smaller = size.div_ceil(2);
                log::warn!("GitHub didn't answer for {size} items, so asking for {smaller}");
                Ok((self.query(&request(smaller))?, smaller))
            }
        }
    }
}

#[cfg(test)]
mod tests;
