//! The PRs that mention an Issue, which only the link rules can sort out (ADR 0011). A sync reads them inside
//! a page of Issues, or for a list of Issues by number, and follows an Issue with more through
//! `fetch_cross_references`.

use serde::Deserialize;

use super::GitHub;
use super::issues::{ByNumber, ListedPr, ReadByNumber, query_by_number};
use super::pull_requests::RateLimit;
use crate::error::Error;
use crate::repository::RepositoryName;

/// The fields of one page of an Issue's cross-references.
pub(super) const REFERENCE_FIELDS: &str = "pageInfo { hasPreviousPage startCursor } nodes { ... on \
     CrossReferencedEvent { source { __typename ... on PullRequest { id number updatedAt repository { \
     nameWithOwner } } } } }";

/// The cross-references of one Issue, and the cursor of the older ones if a page didn't hold them all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueReferences {
    pub number: u32,
    /// Only PRs of this Repository, the newest first.
    pub listed: Vec<ListedPr>,
    pub next: Option<String>,
}

/// An answer to a read of Issues by number.
#[derive(Debug)]
pub struct FetchedReferences {
    pub references: Vec<IssueReferences>,
    /// The count of numbers this answers, after any halving.
    pub asked_for: usize,
    pub rate_limit: RateLimit,
}

#[derive(Deserialize)]
struct NamedRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: RepositoryName,
}

#[derive(Deserialize)]
struct SourcePr {
    #[serde(flatten)]
    listed: ListedPr,
    repository: NamedRepository,
}

/// The Issue or PR that mentioned the Issue.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum Source {
    PullRequest(SourcePr),
    Issue,
}

#[derive(Deserialize)]
struct CrossReferenceEvent {
    source: Source,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelinePage {
    has_previous_page: bool,
    start_cursor: Option<String>,
}

/// One page of an Issue's cross-references as GitHub writes it.
#[derive(Deserialize)]
pub(super) struct ReferenceTimeline {
    #[serde(rename = "pageInfo")]
    page_info: TimelinePage,
    nodes: Vec<Option<CrossReferenceEvent>>,
}

impl ReferenceTimeline {
    /// The PRs of `repository` on this page, the newest first, and the cursor of the older events.
    pub(super) fn read(self, repository: &RepositoryName) -> (Vec<ListedPr>, Option<String>) {
        let listed = self
            .nodes
            .into_iter()
            .rev()
            .flatten()
            .filter_map(|event| match event.source {
                Source::PullRequest(pr) if pr.repository.name_with_owner.is_same(repository) => {
                    Some(pr.listed)
                }
                Source::PullRequest(_) | Source::Issue => None,
            })
            .collect();
        let info = self.page_info;
        (listed, info.start_cursor.filter(|_| info.has_previous_page))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueWire {
    number: u32,
    timeline_items: ReferenceTimeline,
}

fn fetch_references_query(numbers: &[u32]) -> String {
    let fields = format!(
        "number timelineItems(last: 50, itemTypes: [CROSS_REFERENCED_EVENT]) {{ {REFERENCE_FIELDS} }}"
    );
    query_by_number("FetchIssueReferences", numbers, &fields)
}

impl GitHub {
    /// The newest cross-references of the first `size` of `numbers`, or of half that many after a 502 or no
    /// answer. `numbers` holds at least one.
    pub fn fetch_issue_references(
        &self,
        repository: &RepositoryName,
        numbers: &[u32],
        size: usize,
    ) -> Result<FetchedReferences, Error> {
        let read: ReadByNumber<IssueWire> = self.read_by_number(
            repository,
            ByNumber {
                operation: "FetchIssueReferences",
                query: fetch_references_query,
                numbers,
                size,
            },
        )?;
        let references = read
            .issues
            .into_iter()
            .map(|wire| {
                let (listed, next) = wire.timeline_items.read(repository);
                IssueReferences {
                    number: wire.number,
                    listed,
                    next,
                }
            })
            .collect();
        Ok(FetchedReferences {
            references,
            asked_for: read.asked_for,
            rate_limit: read.rate_limit,
        })
    }
}

#[cfg(test)]
mod tests;
