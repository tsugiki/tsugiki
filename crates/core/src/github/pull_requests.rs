//! Finding and fetching PRs on GitHub, and what comes back in the form the rest of Tsugiki reads. What to
//! look for, how much to ask for and when to pause are Tsugiki's choices, and `sync` makes them
//! (ADR 0015).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::activity::{ItemActivity, TimelineWire, pull_request_activity};
use super::{BAD_GATEWAY, CommitSha, GitHub, Login, Request, Unreachable, read};
use crate::conventions::Conventions;
use crate::error::Error;
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
         rateLimit {{ remaining resetAt }} }}"
    )
}

/// The Workbench works out from these lists why a PR is the user's, so a list cut short would hide a PR the
/// searches found. GitHub returns at most 100 at a time, and it limits a PR to 10 assignees. `lastCommit`
/// asks for CI on its own, because CI rollups are slow and `commits` can run to 100. The last 50 items of a
/// timeline are enough for New activity, which has to say that someone did something lately.
pub(super) const PULL_REQUEST_FIELDS: &str = "number title state isDraft updatedAt baseRefName body \
                                   mergeCommit { oid } \
                                   repository { nameWithOwner defaultBranchRef { name } } \
                                   closingIssuesReferences(first: 100) { nodes { number \
                                   repository { nameWithOwner } } } author { login } assignees(first: 20) { nodes { login } } \
                                   reviewRequests(first: 100) { nodes { requestedReviewer { \
                                   ... on User { login } ... on Bot { login } } } } \
                                   latestReviews(first: 100) { nodes { author { login } state } } \
                                   lastCommit: commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } \
                                   commits(first: 100) { totalCount nodes { commit { message \
                                   parents { totalCount } committer { user { login } } } } } \
                                   timelineItems(last: 50, itemTypes: [PULL_REQUEST_COMMIT, ISSUE_COMMENT, \
                                   PULL_REQUEST_REVIEW, READY_FOR_REVIEW_EVENT, HEAD_REF_FORCE_PUSHED_EVENT]) { \
                                   nodes { __typename \
                                   ... on PullRequestCommit { commit { committedDate \
                                   author { name user { login } } committer { name user { login } } } } \
                                   ... on IssueComment { createdAt author { __typename login } bodyText } \
                                   ... on PullRequestReview { submittedAt author { __typename login } } \
                                   ... on ReadyForReviewEvent { createdAt actor { __typename login } } \
                                   ... on HeadRefForcePushedEvent { createdAt actor { __typename login } } } }";

/// The text of a fetch of PRs by ID.
fn fetch_by_ids_query(operation: PrFetch) -> String {
    format!(
        "query {}($ids: [ID!]!) {{ nodes(ids: $ids) {{ ... on PullRequest {{ {PULL_REQUEST_FIELDS} }} }} \
         rateLimit {{ remaining resetAt }} }}",
        operation.name()
    )
}

/// The text of a fetch of one PR by its number.
fn fetch_by_number_query() -> String {
    format!(
        "query {FETCH_ORIGIN}($owner: String!, $name: String!, $number: Int!) {{ \
         repository(owner: $owner, name: $name) {{ pullRequest(number: $number) {{ {PULL_REQUEST_FIELDS} }} }} \
         rateLimit {{ remaining resetAt }} }}"
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

/// GitHub's ID for an item, which a fetch asks for it by.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

/// GitHub's count of the points left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

/// A commit's CI, as GitHub rolls its checks up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CiState {
    Expected,
    Error,
    Failure,
    Pending,
    Success,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Pending,
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
}

/// Someone's latest Review of a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestReview {
    pub author: Login,
    pub state: ReviewState,
}

/// The facts Tsugiki keeps about a PR, as GitHub reports them (ADR 0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestFacts {
    pub number: u32,
    pub title: String,
    pub state: PrState,
    pub is_draft: bool,
    /// Nobody, when the account no longer exists.
    pub author: Option<Login>,
    pub target_branch: String,
    /// Nothing, when the last commit has no checks.
    pub ci: Option<CiState>,
    pub updated_at: OffsetDateTime,
    pub review_requests: Vec<Login>,
    pub latest_reviews: Vec<LatestReview>,
    pub assignees: Vec<Login>,
    /// The Issues in this Repository that GitHub says this PR closes.
    pub closing_issues: Vec<u32>,
    /// The commit a merged PR became. GitHub gives one only for a merged PR.
    pub merge_commit: Option<CommitSha>,
    /// The facts the link rules read on a PR that doesn't target the default branch (ADR 0011).
    pub off_default: Option<OffDefault>,
}

/// The facts only a PR off the default branch needs, for the link rules (ADR 0011).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffDefault {
    /// GitHub ignores closing keywords here, so Tsugiki reads them itself.
    pub description: String,
    /// The commits its description or its commits name in a `(cherry picked from commit <sha>)` line.
    pub cherry_picked_from: Vec<CommitSha>,
    /// The committer of each commit that isn't a merge. `None` for one with no GitHub account or a
    /// commit past the ones Tsugiki reads.
    pub committers: Vec<Option<Login>>,
}

impl PullRequestFacts {
    /// True when a sync needn't fetch this PR again, because it has merged or closed (ADR 0011). A merged PR
    /// with no merge commit came from a Tsugiki that kept none, so a fetch still has facts to bring.
    pub fn is_frozen(&self) -> bool {
        match self.state {
            PrState::Open => false,
            PrState::Closed => true,
            PrState::Merged => self.merge_commit.is_some(),
        }
    }

    /// True when `login` wrote this PR or has a Review on it. A review they haven't submitted isn't one yet.
    pub fn is_written_or_reviewed_by(&self, login: &Login) -> bool {
        self.author.as_ref() == Some(login)
            || self
                .latest_reviews
                .iter()
                .any(|review| review.author == *login && review.state != ReviewState::Pending)
    }

    /// True when a newer `updatedAt` says nothing a Filter or a tree reads has changed, as a merged PR stays
    /// merged. A closed PR can reopen.
    pub fn stays_as_it_is(&self) -> bool {
        self.state == PrState::Merged && self.is_frozen()
    }
}

#[cfg(test)]
impl OffDefault {
    /// Facts off the default branch with `description` and nothing else.
    pub(crate) fn described(description: &str) -> Self {
        Self {
            description: description.to_owned(),
            cherry_picked_from: vec![],
            committers: vec![],
        }
    }
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
struct ReviewerWire {
    login: Option<Login>,
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
    repository: RepositoryWire,
    closing_issues_references: Nodes<ClosingReference>,
    author: Option<Person>,
    assignees: Nodes<Person>,
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
            closing_issues,
            merge_commit: wire
                .merge_commit
                .filter(|_| wire.state == PrState::Merged)
                .and_then(|commit| CommitSha::parse(&commit.oid)),
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
            let asked: Vec<&str> = ids.iter().take(size).map(|id| id.0.as_str()).collect();
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
    /// how many the answer is for.
    pub(super) fn query_at_size<'a, D: DeserializeOwned>(
        &self,
        size: usize,
        request: impl Fn(usize) -> Request<'a>,
    ) -> Result<(D, usize), Error> {
        match self.transport.post(&self.token, &request(size)) {
            Ok(reply) if reply.status != BAD_GATEWAY => Ok((read(&reply)?, size)),
            Ok(_) | Err(Unreachable) => {
                let smaller = size.div_ceil(2);
                log::warn!("GitHub didn't answer for {size} items, so asking for {smaller}");
                Ok((self.query(&request(smaller))?, smaller))
            }
        }
    }
}

#[cfg(test)]
mod tests;
