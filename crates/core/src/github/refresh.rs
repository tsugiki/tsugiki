//! A refresh's questions (ADR 0015). One REST request says whether anything changed, one GraphQL request
//! lists what did, and a probe reads what changes without moving `updatedAt`. Together they cost a few
//! points.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::filters::{FOUND_FIELDS, FilterPage, FilterSearch, SearchWire};
use super::issues::{IssueFacts, IssueWire};
use super::pull_requests::{
    CiState, LastCommitNode, LatestReview, LatestReviewWire, NodeId, Nodes, PageRequest,
    PullRequestFacts, RateLimit, ReviewRequestWire, ci_of, requested_logins, reviews_of,
};
use super::{ConditionalGet, GitHub, Login, Request, Unreachable};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListPlace {
    /// The next page to read, from this cursor. The first page has none.
    Next(Option<String>),
    Finished,
}

impl ListPlace {
    pub fn is_finished(&self) -> bool {
        *self == Self::Finished
    }

    fn after(&self) -> Option<&str> {
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
    fn after_answer(&self, page: PageInfo) -> Self {
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
    /// The count of numbers it asked for, which a retry can make smaller than the batch.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProbeWire {
    number: u32,
    review_requests: Nodes<ReviewRequestWire>,
    latest_reviews: Nodes<LatestReviewWire>,
    last_commit: Nodes<LastCommitNode>,
}

impl From<ProbeWire> for PrProbe {
    fn from(wire: ProbeWire) -> Self {
        Self {
            number: wire.number,
            ci: ci_of(wire.last_commit),
            review_requests: requested_logins(wire.review_requests),
            latest_reviews: reviews_of(wire.latest_reviews),
        }
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
         nodes {{ number title updatedAt author {{ login }} }} }} \
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
                 lastCommit: commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state }} }} }} }} }}"
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
        match self.transport.get(&self.token, &request) {
            Ok(reply) if reply.status == NOT_MODIFIED => ListCheck::Same,
            Ok(reply) if reply.status == OK => {
                ListCheck::Moved(reply.etag.as_deref().map(Etag::new))
            }
            Ok(reply) => {
                log::warn!("GitHub answered a list check with status {}", reply.status);
                ListCheck::Moved(None)
            }
            Err(Unreachable) => ListCheck::Moved(None),
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
        struct Listed<T> {
            page_info: PageInfo,
            nodes: Vec<Option<T>>,
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
        Ok(PollAnswer {
            open_items: polled.open_issues.total_count + polled.open_prs.total_count,
            issues: polled
                .issues
                .nodes
                .into_iter()
                .flatten()
                .map(IssueFacts::from)
                .collect(),
            next_issues: asked.issues.after_answer(polled.issues.page_info),
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
        Ok(ProbedBatch {
            probes: data
                .repository
                .into_values()
                .flatten()
                .map(PrProbe::from)
                .collect(),
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
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use crate::github::pull_requests::{PrState, ReviewState};
    use crate::github::replay::Replayer;
    use crate::github::testing::{Scripted, reply_ok, token};
    use crate::github::{ConditionalReply, Reply, Transport};
    use crate::token::Token;
    use std::path::Path;
    use std::sync::mpsc::{self, Sender};

    const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;
    const START: ListPlace = ListPlace::Next(None);
    const NEWER: &str = "2026-11-04T10:30:00Z";
    const OLDER: &str = "2026-11-04T09:00:00Z";

    fn cpython() -> RepositoryName {
        RepositoryName::parse("python/cpython").unwrap()
    }

    fn github(script: Scripted) -> GitHub {
        GitHub::new(Box::new(script), token())
    }

    fn since() -> OffsetDateTime {
        OffsetDateTime::parse("2026-11-04T10:00:00Z", &Rfc3339).unwrap()
    }

    fn filter(text: &str) -> FilterQuery {
        text.to_owned().try_into().unwrap()
    }

    fn pr(number: u32, updated_at: &str) -> Value {
        json!({ "id": format!("id{number}"), "number": number, "updatedAt": updated_at })
    }

    fn issue(number: u32) -> Value {
        json!({
            "number": number,
            "title": format!("Issue {number}"),
            "updatedAt": NEWER,
            "author": { "login": "some-author" },
        })
    }

    fn list(nodes: &[Value], next: Option<&str>) -> Value {
        json!({
            "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next },
            "nodes": nodes,
        })
    }

    /// An answer with these lists, and these searches as fields next to them.
    fn polled(issues: &Value, prs: &Value, searches: &str) -> Result<Reply, Unreachable> {
        let repository = json!({
            "openIssues": { "totalCount": 7 },
            "openPrs": { "totalCount": 5 },
            "issues": issues,
            "pullRequests": prs,
        });
        reply_ok(&format!(
            r#"{{"data":{{"repository":{repository},{searches}{RATE}}}}}"#
        ))
    }

    fn poll_of<'a>(
        issues: &'a ListPlace,
        prs: &'a ListPlace,
        filters: &'a [FilterQuery],
    ) -> PollRequest<'a> {
        PollRequest {
            since: since(),
            issues,
            pull_requests: prs,
            filters,
            size: 50,
        }
    }

    #[test]
    fn a_poll_asks_for_each_list_and_narrows_each_filter_to_the_same_time() {
        let filters = [filter("is:open OR label:bug"), filter("author:@me")];
        let searches = format!(
            r#""f0":{0},"f1":{0},"#,
            json!({ "issueCount": 0, "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] })
        );
        let (script, asked) =
            Scripted::new(vec![polled(&list(&[], None), &list(&[], None), &searches)]);

        let answer = github(script)
            .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
            .unwrap();

        let (operation, sent) = asked.recv().unwrap();
        assert_eq!(operation, "RefreshRepository");
        assert_eq!(sent["owner"], "python");
        assert_eq!(sent["name"], "cpython");
        assert_eq!(sent["since"], "2026-11-04T10:00:00Z");
        assert_eq!(sent["issues"], 50);
        assert_eq!(sent["prs"], 50);
        assert_eq!(
            sent["filter0"],
            "repo:python/cpython (is:open OR label:bug) updated:>=2026-11-04T10:00:00Z"
        );
        assert_eq!(
            sent["filter1"],
            "repo:python/cpython (author:@me) updated:>=2026-11-04T10:00:00Z"
        );
        assert_eq!(answer.open_items, 12);
        assert_eq!(answer.filters.len(), 2);
        assert_eq!(answer.filters[1].0, filter("author:@me"));
    }

    #[test]
    fn a_poll_with_no_filters_asks_for_no_searches() {
        assert!(!poll_query(0).contains("search("));
        assert!(!poll_query(0).contains("$first"));
        assert!(poll_query(2).contains("$first: Int!"));
        assert!(poll_query(2).contains("f1: search(query: $filter1"));
    }

    #[test]
    fn a_poll_reads_what_changed() {
        let filters = [filter("a")];
        let hit = json!({
            "__typename": "Issue",
            "number": 3,
            "updatedAt": NEWER,
            "repository": { "nameWithOwner": "python/cpython" },
        });
        let searches = format!(
            r#""f0":{},"#,
            json!({
                "issueCount": 1,
                "pageInfo": { "hasNextPage": true, "endCursor": "more" },
                "nodes": [hit],
            })
        );
        let (script, _asked) = Scripted::new(vec![polled(
            &list(&[issue(1), Value::Null], None),
            &list(&[pr(2, NEWER), Value::Null], None),
            &searches,
        )]);

        let answer = github(script)
            .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
            .unwrap();

        assert_eq!(answer.issues[0].number, 1);
        assert_eq!(answer.issues[0].title, "Issue 1");
        assert_eq!(answer.pull_requests[0].number, 2);
        assert_eq!(answer.next_issues, ListPlace::Finished);
        assert_eq!(answer.next_pull_requests, ListPlace::Finished);
        let (_, page) = &answer.filters[0];
        assert_eq!(page.hits[0].number, 3);
        assert_eq!(page.next.as_deref(), Some("more"));
    }

    #[test]
    fn a_list_with_more_pages_says_where_to_carry_on() {
        let (script, _asked) = Scripted::new(vec![polled(
            &list(&[issue(1)], Some("issues")),
            &list(&[pr(2, NEWER)], Some("prs")),
            "",
        )]);

        let answer = github(script)
            .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
            .unwrap();

        assert_eq!(
            answer.next_issues,
            ListPlace::Next(Some("issues".to_owned()))
        );
        assert_eq!(
            answer.next_pull_requests,
            ListPlace::Next(Some("prs".to_owned()))
        );
    }

    #[test]
    fn the_first_pr_older_than_the_poll_ends_the_list() {
        let (script, _asked) = Scripted::new(vec![polled(
            &list(&[], None),
            &list(&[pr(2, NEWER), pr(3, OLDER)], Some("more")),
            "",
        )]);

        let answer = github(script)
            .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
            .unwrap();

        let numbers: Vec<u32> = answer.pull_requests.iter().map(|pr| pr.number).collect();
        assert_eq!(numbers, [2]);
        assert_eq!(answer.next_pull_requests, ListPlace::Finished);
    }

    #[test]
    fn a_list_already_read_asks_for_nothing_and_stays_finished() {
        let next = ListPlace::Next(Some("cursor".to_owned()));
        let (script, asked) = Scripted::new(vec![polled(
            &list(&[], Some("ignored")),
            &list(&[], None),
            "",
        )]);

        let answer = github(script)
            .poll_repository(&cpython(), &poll_of(&ListPlace::Finished, &next, &[]))
            .unwrap();

        let (_, sent) = asked.recv().unwrap();
        assert_eq!(sent["issues"], 0);
        assert_eq!(sent["issuesAfter"], Value::Null);
        assert_eq!(sent["prs"], 50);
        assert_eq!(sent["prsAfter"], "cursor");
        assert_eq!(answer.next_issues, ListPlace::Finished);
        assert!(answer.next_issues.is_finished());
    }

    #[test]
    fn a_poll_github_refuses_says_so() {
        let refused = reply_ok(r#"{"data":null,"errors":[{"message":"Bad query"}]}"#);
        let (script, _asked) = Scripted::new(vec![refused]);

        let problem = github(script)
            .poll_repository(&cpython(), &poll_of(&START, &START, &[]))
            .unwrap_err();

        assert_eq!(problem.kind, ErrorKind::QueryRefused);
    }

    /// Answers each list check the same way, and says what it was asked.
    struct Lists {
        answer: Result<ConditionalReply, Unreachable>,
        asked: Sender<(String, Option<String>)>,
    }

    impl Transport for Lists {
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            Err(Unreachable)
        }

        fn get(
            &self,
            _: &Token,
            request: &ConditionalGet<'_>,
        ) -> Result<ConditionalReply, Unreachable> {
            let seen = (request.path.to_owned(), request.etag.map(str::to_owned));
            drop(self.asked.send(seen));
            self.answer.clone()
        }
    }

    fn listing(answer: Result<ConditionalReply, Unreachable>, tag: Option<&Etag>) -> ListCheck {
        let lists = Lists {
            answer,
            asked: mpsc::channel().0,
        };
        GitHub::new(Box::new(lists), token()).check_list(&cpython(), tag)
    }

    fn replied(status: u16, etag: Option<&str>) -> Result<ConditionalReply, Unreachable> {
        Ok(ConditionalReply {
            status,
            etag: etag.map(str::to_owned),
        })
    }

    #[test]
    fn a_list_check_sends_the_tag_it_was_given() {
        let (sent, asked) = mpsc::channel();
        let lists = Lists {
            answer: replied(304, None),
            asked: sent,
        };
        let tag = Etag::new("W/\"abc\"");

        let check = GitHub::new(Box::new(lists), token()).check_list(&cpython(), Some(&tag));

        assert_eq!(check, ListCheck::Same);
        assert_eq!(
            asked.recv().unwrap(),
            (
                "/repos/python/cpython/issues?state=all&sort=updated&per_page=1".to_owned(),
                Some("W/\"abc\"".to_owned())
            )
        );
    }

    #[test]
    fn a_list_check_says_nothing_about_graphql() {
        let lists = Lists {
            answer: replied(304, None),
            asked: mpsc::channel().0,
        };
        let github = GitHub::new(Box::new(lists), token());

        assert_eq!(github.check_list(&cpython(), None), ListCheck::Same);
        assert_eq!(github.viewer().unwrap_err().kind, ErrorKind::Offline);
    }

    #[test]
    fn a_list_that_moved_gives_its_new_tag() {
        let check = listing(replied(200, Some("\"new\"")), None);
        assert_eq!(check, ListCheck::Moved(Some(Etag::new("\"new\""))));
    }

    #[test]
    fn a_list_check_that_fails_counts_as_moved() {
        assert_eq!(listing(replied(200, None), None), ListCheck::Moved(None));
        assert_eq!(listing(replied(500, None), None), ListCheck::Moved(None));
        assert_eq!(listing(Err(Unreachable), None), ListCheck::Moved(None));
    }

    #[test]
    fn a_transport_with_no_rest_counts_as_unreachable() {
        let (script, _asked) = Scripted::new(Vec::new());
        assert_eq!(
            github(script).check_list(&cpython(), None),
            ListCheck::Moved(None)
        );
    }

    fn probe_answer(fields: &Value) -> Result<Reply, Unreachable> {
        reply_ok(&format!(r#"{{"data":{{"repository":{fields},{RATE}}}}}"#))
    }

    fn probed(number: u32) -> Value {
        json!({
            "number": number,
            "reviewRequests": { "nodes": [{ "requestedReviewer": { "login": "some-reviewer" } }] },
            "latestReviews": { "nodes": [{ "author": { "login": "some-author" }, "state": "APPROVED" }] },
            "lastCommit": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }] },
        })
    }

    #[test]
    fn a_probe_reads_ci_and_reviews_and_skips_a_pr_that_has_gone() {
        let fields = json!({ "p1": probed(1), "p2": null });
        let (script, asked) = Scripted::new(vec![probe_answer(&fields)]);

        let batch = github(script)
            .probe_pull_requests(&cpython(), &[1, 2, 3], 2)
            .unwrap();

        assert_eq!(asked.recv().unwrap().0, "ProbePullRequests");
        assert_eq!(batch.asked_for, 2);
        assert_eq!(
            batch.probes,
            [PrProbe {
                number: 1,
                ci: Some(CiState::Success),
                review_requests: vec![Login::new("some-reviewer")],
                latest_reviews: vec![LatestReview {
                    author: Login::new("some-author"),
                    state: ReviewState::Approved,
                }],
            }]
        );
    }

    #[test]
    fn a_probe_asks_for_half_after_a_bad_gateway() {
        let bad_gateway = Ok(Reply {
            status: 502,
            reset: None,
            body: String::new(),
        });
        let (script, _asked) =
            Scripted::new(vec![bad_gateway, probe_answer(&json!({ "p1": probed(1) }))]);

        let batch = github(script)
            .probe_pull_requests(&cpython(), &[1, 2], 2)
            .unwrap();

        assert_eq!(batch.asked_for, 1);
    }

    #[test]
    fn a_probe_of_each_number_asks_for_it_by_name() {
        let query = probe_query(&[5, 6]);
        assert!(query.contains("p5: pullRequest(number: 5)"));
        assert!(query.contains("p6: pullRequest(number: 6)"));
    }

    #[test]
    fn a_probe_changes_only_what_it_read() {
        let stored = PullRequestFacts {
            number: 1,
            title: "A title".to_owned(),
            state: PrState::Open,
            is_draft: true,
            author: None,
            target_branch: "main".to_owned(),
            ci: Some(CiState::Pending),
            updated_at: since(),
            review_requests: Vec::new(),
            latest_reviews: Vec::new(),
            assignees: vec![Login::new("some-assignee")],
            closing_issues: vec![9],
            merge_commit: None,
            off_default: None,
        };
        let probe = PrProbe {
            number: 1,
            ci: Some(CiState::Failure),
            review_requests: vec![Login::new("some-reviewer")],
            latest_reviews: Vec::new(),
        };

        let after = probe.applied_to(&stored);

        assert_eq!(
            after,
            PullRequestFacts {
                ci: Some(CiState::Failure),
                review_requests: vec![Login::new("some-reviewer")],
                ..stored
            }
        );
    }

    fn fixtures() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github")
    }

    /// The text of the recorded query in `<operation>.json`, which says what a test has to ask to replay it.
    fn recorded_query(operation: &str) -> String {
        let text = std::fs::read_to_string(fixtures().join(format!("{operation}.json"))).unwrap();
        let recording: Value = serde_json::from_str(&text).unwrap();
        recording["query"].as_str().unwrap().to_owned()
    }

    #[test]
    fn the_recorded_poll_reads_as_a_poll() {
        let filters = [filter("a"), filter("b")];
        let github = GitHub::new(Box::new(Replayer::new(fixtures())), token());

        let answer = github
            .poll_repository(&cpython(), &poll_of(&START, &START, &filters))
            .unwrap();

        assert!(answer.open_items > 0);
        assert_eq!(answer.filters.len(), 2);
    }

    #[test]
    fn the_recorded_probe_reads_as_a_probe() {
        let query = recorded_query("ProbePullRequests");
        let numbers: Vec<u32> = query
            .split(" p")
            .skip(1)
            .filter_map(|field| field.split_once(':')?.0.parse().ok())
            .collect();
        let github = GitHub::new(Box::new(Replayer::new(fixtures())), token());

        let batch = github
            .probe_pull_requests(&cpython(), &numbers, numbers.len())
            .unwrap();

        assert!(!batch.probes.is_empty());
    }
}
