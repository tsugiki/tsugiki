//! Finding and fetching PRs on GitHub, and what comes back in the form the rest of Tsugiki reads. What to
//! look for, how much to ask for and when to pause are Tsugiki's choices, and `sync.rs` makes them
//! (ADR 0015).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;
use specta::Type;
use time::OffsetDateTime;

use super::{BAD_GATEWAY, GitHub, Login, Request, Unreachable, read};
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
/// searches found. GitHub returns at most 100 at a time, and it limits a PR to 10 assignees.
const FETCH_PULL_REQUESTS: &str = "query FetchPullRequests($ids: [ID!]!) { nodes(ids: $ids) { \
                                   ... on PullRequest { number title state isDraft updatedAt baseRefName \
                                   author { login } assignees(first: 20) { nodes { login } } \
                                   reviewRequests(first: 100) { nodes { requestedReviewer { \
                                   ... on User { login } ... on Bot { login } } } } \
                                   latestReviews(first: 100) { nodes { author { login } state } } \
                                   commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } } } \
                                   rateLimit { remaining resetAt } }";

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
    pub pull_requests: Vec<PullRequest>,
    /// The number of IDs it asked for, which is fewer than offered after a retry.
    pub asked_for: usize,
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
    pub review_requests: Vec<Login>,
    pub latest_reviews: Vec<LatestReview>,
    pub assignees: Vec<Login>,
}

/// A PR as a fetch returns it. The time only orders rows, so it stays out of the facts a screen reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub updated_at: OffsetDateTime,
    pub facts: PullRequestFacts,
}

#[derive(Deserialize)]
struct Nodes<T> {
    /// GitHub's schema allows a null for anything in a list, such as an item the token can't see.
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
struct Person {
    login: Login,
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
struct CommitNode {
    commit: CommitWire,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommitWire {
    status_check_rollup: Option<RollupWire>,
}

#[derive(Deserialize)]
struct RollupWire {
    state: CiState,
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
    author: Option<Person>,
    assignees: Nodes<Person>,
    review_requests: Nodes<ReviewRequestWire>,
    latest_reviews: Nodes<LatestReviewWire>,
    commits: Nodes<CommitNode>,
}

impl From<PullRequestWire> for PullRequest {
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
            .commits
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
        Self {
            updated_at: wire.updated_at,
            facts: PullRequestFacts {
                number: wire.number,
                title: wire.title,
                state: wire.state,
                is_draft: wire.is_draft,
                author: wire.author.map(|person| person.login),
                target_branch: wire.base_ref_name,
                ci,
                review_requests,
                latest_reviews,
                assignees,
            },
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
            query: &query,
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
    pub fn fetch_pull_requests(&self, ids: &[NodeId], size: usize) -> Result<FetchedBatch, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            nodes: Vec<Option<PullRequestWire>>,
            rate_limit: RateLimit,
        }
        let (data, asked_for): (Data, usize) = self.query_at_size(size.min(ids.len()), |size| {
            let asked: Vec<&str> = ids.iter().take(size).map(|id| id.0.as_str()).collect();
            Request {
                operation: "FetchPullRequests",
                query: FETCH_PULL_REQUESTS,
                variables: json!({ "ids": asked }),
            }
        })?;
        Ok(FetchedBatch {
            pull_requests: data
                .nodes
                .into_iter()
                .flatten()
                .map(PullRequest::from)
                .collect(),
            asked_for,
            rate_limit: data.rate_limit,
        })
    }

    /// Asks for `size` items, and after a 502 or no answer asks once more for half as many (ADR 0015). Says
    /// how many the answer is for.
    fn query_at_size<'a, D: DeserializeOwned>(
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
            "author": { "login": "vstinner" },
            "assignees": { "nodes": [{ "login": "hugovk" }] },
            "reviewRequests": { "nodes": [
                { "requestedReviewer": { "login": "Octocat" } },
                { "requestedReviewer": {} },
                { "requestedReviewer": null },
            ] },
            "latestReviews": { "nodes": [
                { "author": { "login": "pablogsal" }, "state": "CHANGES_REQUESTED" },
                { "author": null, "state": "APPROVED" },
            ] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "FAILURE" } } }] },
        })
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
        assert_eq!(recorded("FetchPullRequests"), FETCH_PULL_REQUESTS);
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
            .fetch_pull_requests(&ids(&["a"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests.len(), 1);
        let pr = &batch.pull_requests[0];
        assert_eq!(
            pr.facts,
            PullRequestFacts {
                number: 140_124,
                title: "gh-1: Fix it".to_owned(),
                state: PrState::Open,
                is_draft: true,
                author: Some(Login::new("vstinner")),
                target_branch: "main".to_owned(),
                ci: Some(CiState::Failure),
                review_requests: vec![Login::new("octocat")],
                latest_reviews: vec![LatestReview {
                    author: Login::new("pablogsal"),
                    state: ReviewState::ChangesRequested,
                }],
                assignees: vec![Login::new("hugovk")],
            }
        );
    }

    #[test]
    fn a_pr_with_no_checks_and_no_author_says_so() {
        let mut pr = pr_json();
        pr["author"] = json!(null);
        pr["commits"] = json!({ "nodes": [{ "commit": { "statusCheckRollup": null } }] });
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([pr])))]);

        let batch = github(script)
            .fetch_pull_requests(&ids(&["a"]), 25)
            .unwrap();

        assert_eq!(batch.pull_requests.len(), 1);
        let pr = &batch.pull_requests[0];
        assert_eq!(pr.facts.author, None);
        assert_eq!(pr.facts.ci, None);
    }

    #[test]
    fn an_id_github_no_longer_knows_is_left_out() {
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([null])))]);

        let batch = github(script)
            .fetch_pull_requests(&ids(&["gone"]), 25)
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
            .fetch_pull_requests(&ids(&["gone"]), 25)
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
            .fetch_pull_requests(&ids(&["gone", "theirs"]), 25)
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
            "author": null,
            "assignees": { "nodes": [null, { "login": "octocat" }] },
            "reviewRequests": { "nodes": [null] },
            "latestReviews": { "nodes": [null] },
            "commits": { "nodes": [null] },
        });
        let (script, _asked) = Scripted::new(vec![reply_ok(&fetch_body(&json!([node])))]);

        let batch = github(script)
            .fetch_pull_requests(&ids(&["a"]), 25)
            .unwrap();

        let facts = &batch.pull_requests[0].facts;
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
            .fetch_pull_requests(&thirty_ids(), 25)
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
            .fetch_pull_requests(&thirty_ids(), 25)
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
            .fetch_pull_requests(&ids(&["a"]), 25)
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
            .fetch_pull_requests(&ids(&["a"]), 25)
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::Unauthorized);
        drop(asked.recv().unwrap());
        assert!(asked.try_recv().is_err());
    }
}
