//! A Filter's search: IDs first, so a sync fetches only what it's missing (ADR 0015). GitHub lists at most
//! 1,000 results for one search, so a Filter that reaches more is read through ranges of the day each item
//! was created.

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::{Date, Duration, OffsetDateTime};

use super::pull_requests::{PageRequest, RateLimit};
use super::refresh::{PageInfo, github_time};
use super::{GitHub, RATE_LIMIT_FIELDS, Request};
use crate::error::Error;
use crate::facts::inbox_search::{InboxSearch, RowKind, SearchHit};
use crate::facts::pull_requests::NodeId;
use crate::filters::FilterQuery;
use crate::repository::RepositoryName;

/// Names the search, and so its recording: one recording answers each Filter and each typed search.
const SEARCH_FILTER: &str = "SearchFilter";

/// The results one page of a typed Inbox search asks for: GitHub's most (#465).
pub(super) const TYPED_PAGE: usize = 100;

/// The pages a typed Inbox search reads at most, so 1,000 results (#465).
const TYPED_PAGES: usize = 10;
/// What a search reads about each item it finds.
pub(super) const FOUND_FIELDS: &str = "__typename \
     ... on Issue { number updatedAt repository { nameWithOwner } } \
     ... on PullRequest { id number updatedAt repository { nameWithOwner } }";

fn search_filter_query() -> String {
    format!(
        "query {SEARCH_FILTER}($query: String!, $first: Int!, $after: String) {{ \
         search(query: $query, type: ISSUE_ADVANCED, first: $first, after: $after) {{ \
         issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ {FOUND_FIELDS} }} }} \
         {RATE_LIMIT_FIELDS} }}"
    )
}

/// The days from `from` to `to`, both included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// The search GitHub runs for a query typed in the Inbox (#465): `repo:` brackets it, Tsugiki adds `is:open`
/// as the cache does, and `sort:updated-desc` unless the query sorts itself.
pub(super) fn typed_text(repository: &RepositoryName, text: &str) -> String {
    let mut names_state = false;
    let mut sorts = false;
    for token in text.split_whitespace() {
        names_state |= crate::filters::names_a_state(token);
        sorts |= crate::filters::names_a_sort(token);
    }
    let mut query = format!("repo:{repository} ({})", text.trim());
    if !names_state {
        query.push_str(" is:open");
    }
    if !sorts {
        query.push_str(" sort:updated-desc");
    }
    query
}

/// The handle a fetch needs for an item the search found.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FilterHitKind {
    /// A fetch asks for an Issue by its number.
    Issue,
    PullRequest(NodeId),
}

/// An item a Filter found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        let found = self.nodes.into_iter().flatten();
        FilterPage {
            total: self.issue_count,
            hits: found
                .filter_map(|found| found.into_hit(repository))
                .collect(),
            next: self.page_info.next(),
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

    /// Every result the query `text` typed in the Inbox finds in `repository`, in GitHub's order, and how
    /// many it reaches (#465). Reads ten light pages of a hundred at most, GitHub's 1,000 results.
    pub fn search_inbox(
        &self,
        repository: &RepositoryName,
        text: &str,
    ) -> Result<InboxSearch, Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Data {
            search: SearchWire,
            rate_limit: RateLimit,
        }
        let query = typed_text(repository, text);
        let request = search_filter_query();
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut total = 0;
        let mut after: Option<String> = None;
        for _ in 0..TYPED_PAGES {
            let (data, _): (Data, usize) = self.query_at_size(TYPED_PAGE, |first| Request {
                operation: SEARCH_FILTER,
                query: request.as_str().into(),
                variables: json!({
                    "query": query.clone(),
                    "first": first,
                    "after": after.clone(),
                }),
            })?;
            let page = data.search.into_page(repository, data.rate_limit);
            total = page.total;
            // A result GitHub lists on two pages, because the item moved between their reads under
            // `sort:updated-desc`, is one result (#465).
            for found in page.hits {
                let listed = hit(found);
                if hits.iter().any(|seen| seen.number == listed.number) {
                    continue;
                }
                hits.push(listed);
            }
            match page.next {
                Some(next) => after = Some(next),
                None => {
                    return Ok(InboxSearch {
                        hits,
                        total,
                        cut_off: false,
                    });
                }
            }
        }
        Ok(InboxSearch {
            hits,
            total,
            cut_off: true,
        })
    }
}

/// A result of a typed search, as GitHub's page holds it.
fn hit(hit: FilterHit) -> SearchHit {
    let kind = match hit.kind {
        FilterHitKind::Issue => RowKind::Issue,
        FilterHitKind::PullRequest(_) => RowKind::PullRequest,
    };
    SearchHit {
        number: hit.number,
        kind,
        updated_at: hit.updated_at,
    }
}

#[cfg(test)]
mod tests;
