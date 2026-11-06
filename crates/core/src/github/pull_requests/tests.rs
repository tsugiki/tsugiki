use super::*;
use crate::error::ErrorKind;
use crate::github::Reply;
use crate::github::Unreachable;
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
        "timelineItems": { "nodes": [] },
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

    assert_eq!(fetched.pull_request.unwrap().0.number, 140_124);
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
        date: None,
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
        "timelineItems": { "nodes": [] },
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
    let (script, asked) = Scripted::new(vec![
        Err(Unreachable::Offline),
        reply_ok(&search_body(&["a"], None)),
    ]);

    let page = github(script)
        .search_pull_requests(&MINE, &cpython(), page(None))
        .unwrap();

    assert_eq!(page.ids, ids(&["a"]));
    drop(asked.recv().unwrap());
    assert_eq!(asked.recv().unwrap().1["first"], 25);
}

#[test]
fn a_second_failure_is_the_answer() {
    let (script, _asked) = Scripted::new(vec![bad_gateway(), Err(Unreachable::Offline)]);

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
        date: None,
        body: String::new(),
    })]);

    let refused = github(script)
        .fetch_pull_requests(PrFetch::Work, &ids(&["a"]), 25)
        .unwrap_err();

    assert_eq!(refused.kind, ErrorKind::Unauthorized);
    drop(asked.recv().unwrap());
    assert!(asked.try_recv().is_err());
}
