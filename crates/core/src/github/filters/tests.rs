use super::*;
use crate::github::testing::{RATE_LIMIT, Scripted, filter_page, github, replied};
use crate::github::{Reply, Unreachable};
use crate::testing::cpython;
use serde_json::Value;
use time::Month;

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
    filter_page(3214, nodes.as_array().unwrap(), next, RATE_LIMIT)
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
    let bad_gateway = replied(502, "Bad Gateway");
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

#[test]
fn a_typed_search_brackets_its_repository_and_adds_open_and_a_sort() {
    assert_eq!(
        typed_text(&cpython(), "  fix parser  "),
        "repo:python/cpython (fix parser) is:open sort:updated-desc"
    );
}

#[test]
fn a_typed_search_keeps_its_own_state_and_sort() {
    assert_eq!(
        typed_text(&cpython(), "is:closed fix"),
        "repo:python/cpython (is:closed fix) sort:updated-desc"
    );
    assert_eq!(
        typed_text(&cpython(), "state:merged fix"),
        "repo:python/cpython (state:merged fix) sort:updated-desc"
    );
    assert_eq!(
        typed_text(&cpython(), "-is:open fix"),
        "repo:python/cpython (-is:open fix) sort:updated-desc"
    );
    assert_eq!(
        typed_text(&cpython(), "-state:closed fix"),
        "repo:python/cpython (-state:closed fix) sort:updated-desc"
    );
    assert_eq!(
        typed_text(&cpython(), "fix sort:created-asc"),
        "repo:python/cpython (fix sort:created-asc) is:open"
    );
}

#[test]
fn a_typed_search_reads_every_page_github_lists_in_its_order() {
    let first = json!([issue(7, "python/cpython"), pr(8, "pr-8")]);
    let second = json!([pr(9, "pr-9")]);
    let (script, asked) = Scripted::new(vec![
        page_of(&first, Some("page-2")),
        page_of(&second, None),
    ]);

    let found = github(script).search_inbox(&cpython(), "fix parser").unwrap();

    let read: Vec<(u32, RowKind)> = found
        .hits
        .iter()
        .map(|hit| (hit.number, hit.kind))
        .collect();
    assert_eq!(
        read,
        [
            (7, RowKind::Issue),
            (8, RowKind::PullRequest),
            (9, RowKind::PullRequest)
        ]
    );
    assert_eq!(found.total, 3214);
    assert!(!found.cut_off);

    let asked: Vec<(String, Value)> = asked.try_iter().collect();
    assert_eq!(asked.len(), 2);
    let (operation, sent) = &asked[0];
    assert_eq!(operation, "SearchFilter");
    assert_eq!(
        sent["query"],
        "repo:python/cpython (fix parser) is:open sort:updated-desc"
    );
    assert_eq!(sent["first"], json!(TYPED_PAGE));
    assert_eq!(asked[1].1["after"], "page-2");
}

#[test]
fn a_result_github_lists_on_two_pages_is_read_once() {
    let repeated = issue(7, "python/cpython");
    let (script, _asked) = Scripted::new(vec![
        page_of(&json!([repeated.clone()]), Some("page-2")),
        page_of(&json!([repeated, issue(8, "python/cpython")]), None),
    ]);

    let found = github(script).search_inbox(&cpython(), "fix").unwrap();

    let numbers: Vec<u32> = found.hits.iter().map(|listed| listed.number).collect();
    assert_eq!(numbers, [7, 8], "a result two pages name is one row");
    assert_eq!(found.total, 3214);
}

#[test]
fn a_typed_search_stops_at_github_s_thousand_results() {
    let pages: Vec<_> = (0..10)
        .map(|page| {
            let number = u32::try_from(page).unwrap() + 1;
            page_of(&json!([issue(number, "python/cpython")]), Some("more"))
        })
        .collect();
    let (script, asked) = Scripted::new(pages);

    let found = github(script).search_inbox(&cpython(), "fix").unwrap();

    assert_eq!(asked.try_iter().count(), 10, "ten light pages, no more");
    assert_eq!(found.hits.len(), 10);
    assert!(found.cut_off);
    assert_eq!(found.total, 3214);
}
