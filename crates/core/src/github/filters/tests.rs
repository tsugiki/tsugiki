use super::*;
use crate::github::testing::{Scripted, reply_ok, token};
use crate::github::{Reply, Transport, Unreachable};
use serde_json::Value;
use time::Month;

const RATE: &str = r#""rateLimit":{"remaining":4000,"resetAt":"2026-11-04T12:00:00Z"}"#;

fn cpython() -> RepositoryName {
    RepositoryName::parse("python/cpython").unwrap()
}

fn github(transport: impl Transport + 'static) -> GitHub {
    GitHub::new(Box::new(transport), token())
}

fn query() -> FilterQuery {
    "is:open OR label:bug".to_owned().try_into().unwrap()
}

fn day(year: i32, month: Month, of: u8) -> Date {
    Date::from_calendar_date(year, month, of).unwrap()
}

fn range(from: Date, to: Date) -> CreatedRange {
    CreatedRange { from, to }
}

fn first_page<'a>(query: &'a FilterQuery, created: Option<CreatedRange>) -> FilterSearch<'a> {
    FilterSearch {
        query,
        created,
        since: None,
        page: PageRequest {
            size: 50,
            after: None,
        },
    }
}

fn page_of(nodes: &Value, next: Option<&str>) -> Result<Reply, Unreachable> {
    let info = json!({ "hasNextPage": next.is_some(), "endCursor": next });
    reply_ok(&format!(
        r#"{{"data":{{"search":{{"issueCount":3214,"pageInfo":{info},"nodes":{nodes}}},{RATE}}}}}"#
    ))
}

fn issue(number: u32, repository: &str) -> Value {
    json!({
        "__typename": "Issue",
        "number": number,
        "updatedAt": "2026-11-01T10:00:00Z",
        "repository": { "nameWithOwner": repository },
    })
}

fn pr(number: u32, id: &str) -> Value {
    json!({
        "__typename": "PullRequest",
        "id": id,
        "number": number,
        "updatedAt": "2026-11-02T10:00:00Z",
        "repository": { "nameWithOwner": "python/cpython" },
    })
}

#[test]
fn a_search_stays_in_the_repository_even_with_an_or() {
    let (script, asked) = Scripted::new(vec![page_of(&json!([]), None)]);

    let query = query();
    drop(github(script).search_filter(&cpython(), &first_page(&query, None)));

    let (operation, sent) = asked.recv().unwrap();
    assert_eq!(operation, "SearchFilter");
    assert_eq!(sent["query"], "repo:python/cpython (is:open OR label:bug)");
    assert_eq!(sent["first"], 50);
}

#[test]
fn a_range_narrows_the_search_to_its_days() {
    let (script, asked) = Scripted::new(vec![page_of(&json!([]), None)]);

    let query = query();
    let created = Some(range(
        day(2020, Month::January, 2),
        day(2021, Month::March, 4),
    ));
    drop(github(script).search_filter(&cpython(), &first_page(&query, created)));

    let (_, sent) = asked.recv().unwrap();
    assert_eq!(
        sent["query"],
        "repo:python/cpython (is:open OR label:bug) created:2020-01-02..2021-03-04"
    );
}

#[test]
fn a_page_says_what_it_found_and_how_much_the_search_reaches() {
    let nodes = json!([issue(7, "python/cpython"), null, pr(8, "pr-8")]);
    let (script, _asked) = Scripted::new(vec![page_of(&nodes, Some("more"))]);

    let query = query();
    let page = github(script)
        .search_filter(&cpython(), &first_page(&query, None))
        .unwrap();

    assert_eq!(page.total, 3214);
    assert_eq!(page.next.as_deref(), Some("more"));
    let kinds: Vec<(u32, bool)> = page
        .hits
        .iter()
        .map(|hit| (hit.number, hit.kind == FilterHitKind::Issue))
        .collect();
    assert_eq!(kinds, [(7, true), (8, false)]);
}

#[test]
fn an_item_of_another_repository_is_left_out() {
    let nodes = json!([issue(7, "python/peps"), issue(8, "Python/CPython")]);
    let (script, _asked) = Scripted::new(vec![page_of(&nodes, None)]);

    let query = query();
    let page = github(script)
        .search_filter(&cpython(), &first_page(&query, None))
        .unwrap();

    let numbers: Vec<u32> = page.hits.iter().map(|hit| hit.number).collect();
    assert_eq!(numbers, [8]);
}

#[test]
fn a_page_github_cant_answer_is_asked_for_at_half_size() {
    let bad_gateway = Ok(Reply {
        status: 502,
        reset: None,
        date: None,
        body: "Bad Gateway".to_owned(),
    });
    let (script, asked) = Scripted::new(vec![bad_gateway, page_of(&json!([]), None)]);

    let query = query();
    drop(
        github(script)
            .search_filter(&cpython(), &first_page(&query, None))
            .unwrap(),
    );

    let sizes: Vec<Value> = asked
        .try_iter()
        .map(|(_, sent)| sent["first"].clone())
        .collect();
    assert_eq!(sizes, [json!(50), json!(25)]);
}

#[test]
fn the_whole_range_starts_before_github_and_ends_after_today() {
    let whole = CreatedRange::up_to(day(2026, Month::November, 5));
    assert_eq!(whole.from, day(1970, Month::January, 1));
    assert_eq!(whole.to, day(2026, Month::November, 6));
}

#[test]
fn halves_cover_the_range_without_sharing_a_day() {
    let whole = range(day(2026, Month::January, 1), day(2026, Month::January, 4));
    assert_eq!(
        whole.halves(),
        Some((
            range(day(2026, Month::January, 1), day(2026, Month::January, 2)),
            range(day(2026, Month::January, 3), day(2026, Month::January, 4)),
        ))
    );
}

#[test]
fn two_days_halve_into_one_each_and_one_day_doesnt_halve() {
    let (first, second) = (day(2026, Month::January, 1), day(2026, Month::January, 2));
    assert_eq!(
        range(first, second).halves(),
        Some((range(first, first), range(second, second)))
    );
    assert_eq!(range(first, first).halves(), None);
}
