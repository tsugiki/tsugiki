//! Finding and fetching PRs on GitHub, and what comes back in the form the rest of Tsugiki reads. What to
//! look for, how much to ask for and when to pause are Tsugiki's choices, and `sync.rs` makes them
//! (ADR 0015).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::{BAD_GATEWAY, CommitSha, GitHub, Login, Request, Unreachable, read};
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
/// asks for CI on its own, because CI rollups are slow and `commits` can run to 100.
const PULL_REQUEST_FIELDS: &str = "number title state isDraft updatedAt baseRefName body \
                                   mergeCommit { oid } \
                                   repository { nameWithOwner defaultBranchRef { name } } \
                                   closingIssuesReferences(first: 100) { nodes { number \
                                   repository { nameWithOwner } } } author { login } assignees(first: 20) { nodes { login } } \
                                   reviewRequests(first: 100) { nodes { requestedReviewer { \
                                   ... on User { login } ... on Bot { login } } } } \
                                   latestReviews(first: 100) { nodes { author { login } state } } \
                                   lastCommit: commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } \
                                   commits(first: 100) { totalCount nodes { commit { message \
                                   parents { totalCount } committer { user { login } } } } }";

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrFetch {
    /// The PRs the user's searches found.
    Work,
    /// The PRs an Issue's cross-references list.
    Tree,
}

impl PrFetch {
    fn name(self) -> &'static str {
        match self {
            Self::Work => "FetchPullRequests",
            Self::Tree => "FetchTreePullRequests",
        }
    }
}

/// GitHub's ID for an item, which a fetch asks for it by.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    /// The number of IDs it asked for, which is fewer than offered after a retry.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

/// One Origin PR fetched by number.
#[derive(Debug, PartialEq, Eq)]
pub struct FetchedOrigin {
    /// Nothing, when that number names an Issue or nothing.
    pub pull_request: Option<PullRequestFacts>,
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
struct Nodes<T> {
    /// GitHub's schema allows a null for anything in a list, such as an item the token can't see.
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
pub(super) struct Person {
    pub(super) login: Login,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewRequestWire {
    requested_reviewer: Option<ReviewerWire>,
}

/// A team has no `login`, and a query that asks for none gets an empty object for it.
#[derive(Deserialize)]
struct ReviewerWire {
    login: Option<Login>,
}

#[derive(Deserialize)]
struct LatestReviewWire {
    author: Option<Person>,
    state: ReviewState,
}

#[derive(Deserialize)]
struct LastCommitNode {
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
struct PullRequestWire {
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

impl From<PullRequestWire> for PullRequestFacts {
    fn from(wire: PullRequestWire) -> Self {
        let review_requests = wire
            .review_requests
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|request| request.requested_reviewer?.login)
            .collect();
        let latest_reviews = wire
            .latest_reviews
            .nodes
            .into_iter()
            .flatten()
            .filter_map(|review| {
                let author = review.author?.login;
                Some(LatestReview {
                    author,
                    state: review.state,
                })
            })
            .collect();
        let ci = wire
            .last_commit
            .nodes
            .into_iter()
            .flatten()
            .next()
            .and_then(|node| node.commit.status_check_rollup)
            .map(|rollup| rollup.state);
        let assignees = wire
            .assignees
            .nodes
            .into_iter()
            .flatten()
            .map(|person| person.login)
            .collect();
        let here = wire.repository.name_with_owner;
        // A reference to another Repository's Issue can't put this PR under an Issue here (ADR 0011).
        let closing_issues = wire
            .closing_issues_references
            .nodes
            .into_iter()
            .flatten()
            .filter(|reference| reference.repository.name_with_owner.is_same(&here))
            .map(|reference| reference.number)
            .collect();
        let on_default = wire
            .repository
            .default_branch_ref
            .is_none_or(|branch| branch.name == wire.base_ref_name);
        Self {
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
        }
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
        Ok(FetchedBatch {
            pull_requests: data
                .nodes
                .into_iter()
                .flatten()
                .map(PullRequestFacts::from)
                .collect(),
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
            pull_request: data.repository.pull_request.map(PullRequestFacts::from),
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
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::github::Reply;
    use crate::github::testing::{Scripted, reply_ok, token};
    use serde_json::Value;
    use time::format_description::well_known::Rfc3339;

    const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

    const MINE: PrSearch = PrSearch {
        operation: "SearchMine",
        qualifiers: "is:pr author:@me",
    };

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn page(after: Option<&str>) -> PageRequest<'_> {
        PageRequest { size: 50, after }
    }

    fn github(script: Scripted) -> GitHub {
        GitHub::new(Box::new(script), token())
    }

    fn search_body(ids: &[&str], next: Option<&str>) -> String {
        let nodes: Vec<_> = ids.iter().map(|id| json!({ "id": id })).collect();
        let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
        format!(
            r#"{{"data":{{"search":{{"pageInfo":{info},"nodes":{}}},{RATE}}}}}"#,
            json!(nodes)
        )
    }

    fn ids(names: &[&str]) -> Vec<NodeId> {
        names
            .iter()
            .map(|name| NodeId((*name).to_owned()))
            .collect()
    }

    fn thirty_ids() -> Vec<NodeId> {
        (0..30).map(|n| NodeId(format!("id{n}"))).collect()
    }

    fn pr_json() -> serde_json::Value {
        json!({
            "number": 140_124,
            "title": "gh-1: Fix it",
            "state": "OPEN",
            "isDraft": true,
            "updatedAt": "2026-11-04T10:00:00Z",
            "baseRefName": "main",
            "body": "Fixes #2",
            "repository": { "nameWithOwner": "python/cpython", "defaultBranchRef": { "name": "main" } },
            "closingIssuesReferences": { "nodes": [
                { "number": 7, "repository": { "nameWithOwner": "Python/CPython" } },
                { "number": 8, "repository": { "nameWithOwner": "python/peps" } },
                null,
            ] },
            "author": { "login": "some-author" },
            "assignees": { "nodes": [{ "login": "some-reviewer" }] },
            "reviewRequests": { "nodes": [
                { "requestedReviewer": { "login": "Octocat" } },
                { "requestedReviewer": {} },
                { "requestedReviewer": null },
            ] },
            "latestReviews": { "nodes": [
                { "author": { "login": "some-assignee" }, "state": "CHANGES_REQUESTED" },
                { "author": null, "state": "APPROVED" },
            ] },
            "lastCommit": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE" } } }] },
            "mergeCommit": null,
            "commits": { "totalCount": 0, "nodes": [] },
        })
    }

    /// A commit as a fetch answers with it.
    fn commit_json(committer: Value, parents: u32, message: &str) -> Value {
        json!({ "commit": {
            "message": message,
            "parents": { "totalCount": parents },
            "committer": committer,
        } })
    }

    fn by_user(login: &str) -> Value {
        json!({ "user": { "login": login } })
    }

    /// The facts Tsugiki keeps of `pr` when it comes back from a fetch.
    fn read_back(pr: &Value) -> PullRequestFacts {
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([pr])))]);
        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap();
        batch.pull_requests.into_iter().next().unwrap()
    }

    fn off_default_with(body: &str, total: u32, commits: Vec<Value>) -> OffDefault {
        let mut pr = pr_json();
        pr["baseRefName"] = json!("3.14");
        pr["body"] = json!(body);
        pr["commits"] = json!({ "totalCount": total, "nodes": commits });
        read_back(&pr).off_default.unwrap()
    }

    fn sha(text: &str) -> CommitSha {
        CommitSha::parse(text).unwrap()
    }

    #[test]
    fn a_backports_committers_leave_out_merges() {
        let facts = off_default_with(
            "",
            4,
            vec![
                commit_json(by_user("miss-islington"), 1, ""),
                commit_json(json!({ "user": null }), 2, "Merge branch '3.14'"),
                commit_json(by_user("some-author"), 0, ""),
                commit_json(json!({ "user": null }), 1, ""),
            ],
        );
        assert_eq!(
            facts.committers,
            [
                Some(Login::new("miss-islington")),
                Some(Login::new("some-author")),
                None,
            ]
        );
    }

    #[test]
    fn a_commit_with_no_committer_is_nobodys() {
        let facts = off_default_with("", 1, vec![commit_json(Value::Null, 1, "")]);
        assert_eq!(facts.committers, [None]);
    }

    #[test]
    fn commits_past_the_ones_read_count_as_nobodys() {
        let facts = off_default_with("", 150, vec![commit_json(by_user("miss-islington"), 1, "")]);
        assert_eq!(facts.committers, [Some(Login::new("miss-islington")), None]);
    }

    #[test]
    fn a_null_commit_is_a_commit_tsugiki_didnt_read() {
        let facts = off_default_with("", 1, vec![Value::Null]);
        assert_eq!(facts.committers, [None]);
    }

    #[test]
    fn cherry_pick_lines_come_from_the_description_and_the_commits_once_each() {
        let line = |text: &str| format!("Text\r\n(cherry picked from commit {text})\r\nMore");
        let facts = off_default_with(
            &line("AAAAAAA"),
            2,
            vec![
                commit_json(by_user("some-author"), 1, &line("bbbbbbb")),
                commit_json(by_user("some-author"), 1, &line("aaaaaaa")),
            ],
        );
        assert_eq!(facts.cherry_picked_from, [sha("aaaaaaa"), sha("bbbbbbb")]);
    }

    #[test]
    fn a_line_that_only_looks_like_a_cherry_pick_names_nothing() {
        for text in [
            "cherry picked from commit aaaaaaa",
            "(cherry picked from commit aaaaaaa",
            "(cherry picked from commit xyz1234)",
            "(cherry picked from commit aaa)",
            "See (cherry picked from commit aaaaaaa)",
        ] {
            let facts = off_default_with(text, 0, vec![]);
            assert_eq!(facts.cherry_picked_from, [], "{text}");
        }
    }

    #[test]
    fn a_pr_on_the_default_branch_keeps_nothing_of_its_commits() {
        let mut pr = pr_json();
        pr["commits"] = json!({
            "totalCount": 1,
            "nodes": [commit_json(by_user("some-author"), 1, "(cherry picked from commit aaaaaaa)")],
        });
        assert_eq!(read_back(&pr).off_default, None);
    }

    #[test]
    fn only_a_merged_pr_has_a_merge_commit() {
        let in_state = |state: &str, oid: &str| {
            let mut pr = pr_json();
            pr["state"] = json!(state);
            pr["mergeCommit"] = json!({ "oid": oid });
            read_back(&pr).merge_commit
        };
        let whole = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(in_state("MERGED", whole), Some(sha(whole)));
        assert_eq!(in_state("CLOSED", whole), None);
        assert_eq!(in_state("MERGED", "not a sha"), None);
    }

    #[test]
    fn an_origin_fetched_by_number_comes_back_with_its_facts() {
        let body = format!(
            r#"{{"data":{{"repository":{{"pullRequest":{}}},{RATE}}}}}"#,
            pr_json()
        );
        let (script, asked) = Scripted::new(vec![reply_ok(&body)]);

        let fetched = github(script)
            .fetch_origin_pull_request(&cpython(), 140_124)
            .unwrap();

        assert_eq!(fetched.pull_request.unwrap().number, 140_124);
        assert_eq!(fetched.rate_limit.remaining, 4000);
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "FetchOriginPullRequest");
        assert_eq!(
            variables,
            json!({ "owner": "python", "name": "cpython", "number": 140_124 })
        );
    }

    #[test]
    fn an_origin_number_that_names_no_pr_comes_back_empty() {
        let body = format!(
            r#"{{"data":{{"repository":{{"pullRequest":null}},{RATE}}},"errors":[{{"type":"NOT_FOUND",
                "message":"gone","path":["repository","pullRequest"]}}]}}"#
        );
        let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

        let fetched = github(script)
            .fetch_origin_pull_request(&cpython(), 9)
            .unwrap();

        assert_eq!(fetched.pull_request, None);
    }

    #[test]
    fn a_tree_fetch_has_its_own_recording() {
        let (script, asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([])))]);
        drop(
            github(script)
                .fetch_pull_requests(PrFetch::Tree, &ids(&["a"]), 25)
                .unwrap(),
        );
        assert_eq!(asked.recv().unwrap().0, "FetchTreePullRequests");
        assert!(fetch_by_ids_query(PrFetch::Tree).starts_with("query FetchTreePullRequests("));
    }

    fn fetch_body(nodes: &serde_json::Value) -> String {
        format!(r#"{{"data":{{"nodes":{nodes},{RATE}}}}}"#)
    }

    fn bad_gateway() -> Result<Reply, Unreachable> {
        Ok(Reply {
            status: 502,
            reset: None,
            body: "Bad Gateway".to_owned(),
        })
    }

    #[test]
    fn a_search_says_what_it_found_and_where_the_next_page_starts() {
        let (script, asked) = Scripted::new(vec![reply_ok(&search_body(&["a", "b"], Some("c1")))]);

        let page = github(script)
            .search_pull_requests(&MINE, &cpython(), page(Some("c0")))
            .unwrap();

        assert_eq!(page.ids, ids(&["a", "b"]));
        assert_eq!(page.next.as_deref(), Some("c1"));
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "SearchMine");
        assert_eq!(
            variables,
            json!({
                "query": "repo:python/cpython is:pr author:@me",
                "first": 50,
                "after": "c0",
            })
        );
    }

    #[test]
    fn a_last_page_has_no_next_page_even_with_a_cursor() {
        let body = search_body(&[], None).replace(r#""endCursor":null"#, r#""endCursor":"end""#);
        let (script, _asked) = Scripted::new(vec![reply_ok(&body)]);

        let page = github(script)
            .search_pull_requests(&MINE, &cpython(), page(None))
            .unwrap();

        assert_eq!(page.next, None);
    }

    #[test]
    fn the_recordings_hold_the_queries_the_client_sends() {
        let recorded = |operation: &str| -> String {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("tests/fixtures/github/{operation}.json"));
            let text = std::fs::read_to_string(path).unwrap();
            let recording: serde_json::Value = serde_json::from_str(&text).unwrap();
            recording["query"].as_str().unwrap().to_owned()
        };
        assert_eq!(recorded("SearchOwnPrs"), search_query("SearchOwnPrs"));
        assert_eq!(
            recorded("FetchPullRequests"),
            fetch_by_ids_query(PrFetch::Work)
        );
    }

    #[test]
    fn a_search_opts_into_advanced_search_under_its_own_name() {
        let query = search_query("SearchMine");
        assert!(query.starts_with("query SearchMine("), "{query}");
        assert!(query.contains("type: ISSUE_ADVANCED"), "{query}");
    }

    #[test]
    fn a_fetched_pr_is_read_into_its_facts() {
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([pr_json()])))]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests.len(), 1);
        let pr = &batch.pull_requests[0];
        assert_eq!(
            *pr,
            PullRequestFacts {
                number: 140_124,
                title: "gh-1: Fix it".to_owned(),
                state: PrState::Open,
                is_draft: true,
                author: Some(Login::new("some-author")),
                target_branch: "main".to_owned(),
                ci: Some(CiState::Failure),
                updated_at: OffsetDateTime::parse("2026-11-04T10:00:00Z", &Rfc3339).unwrap(),
                review_requests: vec![Login::new("octocat")],
                latest_reviews: vec![LatestReview {
                    author: Login::new("some-assignee"),
                    state: ReviewState::ChangesRequested,
                }],
                assignees: vec![Login::new("some-reviewer")],
                closing_issues: vec![7],
                merge_commit: None,
                off_default: None,
            }
        );
    }

    /// The description Tsugiki keeps for a PR on `branch`, in a Repository with `default_branch`.
    fn kept_description(branch: &str, default_branch: Value) -> Option<String> {
        let mut pr = pr_json();
        pr["baseRefName"] = json!(branch);
        pr["repository"]["defaultBranchRef"] = default_branch;
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([pr])))]);
        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap();
        batch.pull_requests[0]
            .off_default
            .as_ref()
            .map(|off_default| off_default.description.clone())
    }

    #[test]
    fn only_a_pr_off_the_default_branch_keeps_its_description() {
        let main = || json!({ "name": "main" });
        assert_eq!(kept_description("main", main()), None);
        assert_eq!(
            kept_description("3.14", main()).as_deref(),
            Some("Fixes #2")
        );
        assert_eq!(kept_description("3.14", Value::Null), None);
    }

    #[test]
    fn a_pr_with_no_checks_and_no_author_says_so() {
        let mut pr = pr_json();
        pr["author"] = json!(null);
        pr["lastCommit"] = json!({ "nodes": [{ "commit": { "statusCheckRollup": null } }] });
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([pr])))]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests.len(), 1);
        let pr = &batch.pull_requests[0];
        assert_eq!(pr.author, None);
        assert_eq!(pr.ci, None);
    }

    #[test]
    fn an_id_github_no_longer_knows_is_left_out() {
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([null])))]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["gone"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests, []);
        assert_eq!(batch.asked_for, 1);
    }

    #[test]
    fn a_batch_keeps_the_rest_when_github_no_longer_knows_an_id() {
        // The shape real GitHub sends for an ID it can't resolve.
        let body = r#"{"data":{"nodes":[null],"rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}},
            "errors":[{"type":"NOT_FOUND","path":["nodes",0],"message":"Could not resolve to a node"}]}"#;
        let (script, _asked) = Scripted::new(vec![reply_ok(body)]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["gone"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests, []);
        assert_eq!(batch.asked_for, 1);
    }

    #[test]
    fn a_batch_still_fails_on_any_other_error() {
        let body = r#"{"data":{"nodes":[null]},
            "errors":[{"type":"NOT_FOUND","path":["nodes",0],"message":"Gone"},
                      {"type":"FORBIDDEN","path":["nodes",1],"message":"Not yours"}]}"#;
        let (script, _asked) = Scripted::new(vec![reply_ok(body)]);

        let refused = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["gone", "theirs"]), 25)
            .unwrap_err();

        assert_eq!(refused.message, "GitHub said: Not yours");
    }

    #[test]
    fn a_null_in_a_pull_requests_lists_is_left_out() {
        let node = json!({
            "number": 7,
            "title": "gh-7: Fix it",
            "state": "OPEN",
            "isDraft": false,
            "updatedAt": "2026-11-04T10:00:00Z",
            "baseRefName": "main",
            "body": "",
            "repository": { "nameWithOwner": "python/cpython", "defaultBranchRef": { "name": "main" } },
            "closingIssuesReferences": { "nodes": [null] },
            "author": null,
            "assignees": { "nodes": [null, { "login": "octocat" }] },
            "reviewRequests": { "nodes": [null] },
            "latestReviews": { "nodes": [null] },
            "lastCommit": { "nodes": [null] },
            "mergeCommit": null,
            "commits": { "totalCount": 0, "nodes": [] },
        });
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([node])))]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap();

        let facts = &batch.pull_requests[0];
        assert_eq!(facts.assignees, [Login::new("octocat")]);
        assert_eq!(facts.review_requests, []);
        assert_eq!(facts.latest_reviews, []);
        assert_eq!(facts.ci, None);
    }

    #[test]
    fn a_null_in_a_search_is_left_out() {
        let body = json!({ "data": {
            "search": {
                "pageInfo": { "hasNextPage": false, "endCursor": null },
                "nodes": [null, { "id": "a" }],
            },
            "rateLimit": { "remaining": 4000, "resetAt": "2026-11-04T12:00:00Z" },
        }});
        let (script, _asked) = Scripted::new(vec![reply_ok(&body.to_string())]);

        let page = github(script)
            .search_pull_requests(&MINE, &cpython(), page(None))
            .unwrap();

        assert_eq!(page.ids, ids(&["a"]));
    }

    #[test]
    fn a_fetch_asks_for_one_batch_at_a_time() {
        let (script, asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([])))]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &thirty_ids(), 25)
            .unwrap();

        assert_eq!(batch.asked_for, 25);
        let (operation, variables) = asked.recv().unwrap();
        assert_eq!(operation, "FetchPullRequests");
        assert_eq!(variables["ids"].as_array().unwrap().len(), 25);
    }

    #[test]
    fn after_a_502_a_fetch_asks_once_more_at_half_the_size() {
        let (script, asked) = Scripted::new(vec![
            bad_gateway(),
            reply_ok(&fetch_body(&json!([pr_json()]))),
        ]);

        let batch = github(script)
            .fetch_pull_requests(PrFetch::Work, &thirty_ids(), 25)
            .unwrap();

        assert_eq!(batch.asked_for, 13);
        assert_eq!(batch.pull_requests.len(), 1);
        drop(asked.recv().unwrap());
        assert_eq!(asked.recv().unwrap().1["ids"].as_array().unwrap().len(), 13);
    }

    #[test]
    fn with_no_answer_a_search_asks_once_more_at_half_the_size() {
        let (script, asked) =
            Scripted::new(vec![Err(Unreachable), reply_ok(&search_body(&["a"], None))]);

        let page = github(script)
            .search_pull_requests(&MINE, &cpython(), page(None))
            .unwrap();

        assert_eq!(page.ids, ids(&["a"]));
        drop(asked.recv().unwrap());
        assert_eq!(asked.recv().unwrap().1["first"], 25);
    }

    #[test]
    fn a_second_failure_is_the_answer() {
        let (script, _asked) = Scripted::new(vec![bad_gateway(), Err(Unreachable)]);

        let refused = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::Offline);
    }

    #[test]
    fn a_refused_token_isnt_retried() {
        let (script, asked) = Scripted::new(vec![Ok(Reply {
            status: 401,
            reset: None,
            body: String::new(),
        })]);

        let refused = github(script)
            .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::Unauthorized);
        drop(asked.recv().unwrap());
        assert!(asked.try_recv().is_err());
    }
}
