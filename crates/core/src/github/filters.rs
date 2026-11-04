//! A Filter's search: IDs first, so a sync fetches only what it's missing (ADR 0015). GitHub lists at most
//! 1,000 results for one search, so a Filter that reaches more is read through ranges of the day each item
//! was created.

use serde::Deserialize;
use serde_json::json;
use time::{Date, Duration, OffsetDateTime};

use super::pull_requests::{NodeId, PageRequest, RateLimit};
use super::refresh::github_time;
use super::{GitHub, Request};
use crate::error::Error;
use crate::filters::FilterQuery;
use crate::repository::RepositoryName;

/// Names the search, and so its recording: one recording answers each Filter.
const SEARCH_FILTER: &str = "SearchFilter";
/// What a search reads about each item it finds.
pub(super) const FOUND_FIELDS: &str = "__typename \
     ... on Issue { number updatedAt repository { nameWithOwner } } \
     ... on PullRequest { id number updatedAt repository { nameWithOwner } }";

fn search_filter_query() -> String {
    format!(
        "query {SEARCH_FILTER}($query: String!, $first: Int!, $after: String) {{ \
         search(query: $query, type: ISSUE_ADVANCED, first: $first, after: $after) {{ \
         issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ {FOUND_FIELDS} }} }} \
         rateLimit {{ remaining resetAt }} }}"
    )
}

/// The days from `from` to `to`, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedRange {
    from: Date,
    to: Date,
}

impl CreatedRange {
    /// Every day an item can have been created on. GitHub keeps the dates of imported Issues, so this starts
    /// long before GitHub did. It ends after `today`, in case this computer's clock is behind GitHub's.
    pub fn up_to(today: Date) -> Self {
        Self {
            from: OffsetDateTime::UNIX_EPOCH.date(),
            to: today.saturating_add(Duration::days(1)),
        }
    }

    /// This range in two, the older half first. A single day can't be cut.
    pub fn halves(self) -> Option<(Self, Self)> {
        let days = (self.to - self.from).whole_days();
        if days == 0 {
            return None;
        }
        let end_of_older = self.from.saturating_add(Duration::days(days / 2));
        let older = Self {
            from: self.from,
            to: end_of_older,
        };
        let newer = Self {
            from: end_of_older.saturating_add(Duration::days(1)),
            to: self.to,
        };
        Some((older, newer))
    }
}

/// One page of a Filter's search to ask for.
#[derive(Debug, Clone, Copy)]
pub struct FilterSearch<'a> {
    pub query: &'a FilterQuery,
    pub created: Option<CreatedRange>,
    /// Keeps only what changed since this time, for a refresh.
    pub since: Option<OffsetDateTime>,
    pub page: PageRequest<'a>,
}

impl FilterSearch<'_> {
    /// The search GitHub runs. The brackets keep the user's `OR` inside this Repository (ADR 0015).
    pub(super) fn text(&self, repository: &RepositoryName) -> String {
        let query = self.query.as_str();
        let mut text = format!("repo:{repository} ({query})");
        if let Some(CreatedRange { from, to }) = self.created {
            text.push_str(&format!(" created:{from}..{to}"));
        }
        if let Some(since) = self.since {
            text.push_str(&format!(" updated:>={}", github_time(since)));
        }
        text
    }
}

/// The handle a fetch needs for an item the search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterHitKind {
    /// A fetch asks for an Issue by its number.
    Issue,
    PullRequest(NodeId),
}

/// An item a Filter found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterHit {
    pub number: u32,
    pub updated_at: OffsetDateTime,
    pub kind: FilterHitKind,
}

/// One page of a Filter's search.
#[derive(Debug, PartialEq, Eq)]
pub struct FilterPage {
    /// How many items the whole search reaches, even past what GitHub lists.
    pub total: u32,
    /// Only items of this Repository: a user's own `repo:` can name another one.
    pub hits: Vec<FilterHit>,
    /// The next page's cursor, if there is one.
    pub next: Option<String>,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NamedRepository {
    name_with_owner: RepositoryName,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FoundIssue {
    number: u32,
    #[serde(with = "time::serde::rfc3339")]
    updated_at: OffsetDateTime,
    repository: NamedRepository,
}

#[derive(Deserialize)]
struct FoundPr {
    id: NodeId,
    #[serde(flatten)]
    item: FoundIssue,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Found {
    Issue(FoundIssue),
    PullRequest(FoundPr),
}

impl Found {
    fn into_hit(self, repository: &RepositoryName) -> Option<FilterHit> {
        let (item, kind) = match self {
            Self::Issue(item) => (item, FilterHitKind::Issue),
            Self::PullRequest(pr) => (pr.item, FilterHitKind::PullRequest(pr.id)),
        };
        item.repository
            .name_with_owner
            .is_same(repository)
            .then_some(FilterHit {
                number: item.number,
                updated_at: item.updated_at,
                kind,
            })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

/// GitHub's answer to one search.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SearchWire {
    issue_count: u32,
    page_info: PageInfo,
    nodes: Vec<Option<Found>>,
}

impl SearchWire {
    pub(super) fn into_page(
        self,
        repository: &RepositoryName,
        rate_limit: RateLimit,
    ) -> FilterPage {
        let info = self.page_info;
        let found = self.nodes.into_iter().flatten();
        FilterPage {
            total: self.issue_count,
            hits: found
                .filter_map(|found| found.into_hit(repository))
                .collect(),
            next: info.end_cursor.filter(|_| info.has_next_page),
            rate_limit,
        }
    }
}

impl GitHub {
    /// One page of a Filter's search in `repository`, or half that page after a 502 or no answer.
    pub fn search_filter(
        &self,
        repository: &RepositoryName,
        search: &FilterSearch<'_>,
    ) -> Result<FilterPage, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            search: SearchWire,
            rate_limit: RateLimit,
        }
        let text = search.text(repository);
        let page = search.page;
        let query = search_filter_query();
        let (data, _): (Data, usize) = self.query_at_size(page.size, |first| Request {
            operation: SEARCH_FILTER,
            query: query.as_str().into(),
            variables: json!({ "query": text, "first": first, "after": page.after }),
        })?;
        Ok(data.search.into_page(repository, data.rate_limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
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
    fn a_search_that_fails_twice_is_the_failure() {
        let (script, _asked) = Scripted::new(vec![Err(Unreachable), Err(Unreachable)]);

        let query = query();
        let refused = github(script)
            .search_filter(&cpython(), &first_page(&query, None))
            .unwrap_err();

        assert_eq!(refused.kind, ErrorKind::Offline);
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
}
