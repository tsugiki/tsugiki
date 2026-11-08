//! The user's own searches, which find the PRs a first sync starts from (ADR 0015).

use super::filters::FilterWalk;
use super::{Link, Moved, Resumable, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::github::pull_requests::{NodeId, PageRequest, PrSearch};

/// The user's own items in the order a first sync loads them (ADR 0015). Assigned Issues come with Issues
/// themselves, so these all find PRs. A team's Review request doesn't name the user, so it doesn't count.
pub const WORK_SEARCHES: [PrSearch; 4] = [
    PrSearch {
        operation: "SearchReviewRequests",
        qualifiers: "is:pr is:open user-review-requested:@me",
    },
    PrSearch {
        operation: "SearchReviewedPrs",
        qualifiers: "is:pr is:open reviewed-by:@me",
    },
    PrSearch {
        operation: "SearchOwnPrs",
        qualifiers: "is:pr is:open author:@me",
    },
    PrSearch {
        operation: "SearchAssignedPrs",
        qualifiers: "is:pr is:open assignee:@me",
    },
];

pub(super) fn search(
    link: &Link<'_>,
    next_search: &mut usize,
    cursor: &mut Option<String>,
    found: &mut Vec<NodeId>,
) -> Result<Moved, Error> {
    let Some(search) = WORK_SEARCHES.get(*next_search) else {
        link.store
            .keep_only_filters(link.repository, link.filters.to_vec())?;
        let walk = FilterWalk::new(link.filters, std::mem::take(found));
        let next = Resumable::SearchingFilters(walk).into();
        return Ok((Step::Moved, Some(next)));
    };
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: cursor.as_deref(),
    };
    let page = link
        .github
        .search_pull_requests(search, link.repository, asked)?;
    for id in page.ids {
        if !found.contains(&id) {
            found.push(id);
        }
    }
    *cursor = page.next;
    if cursor.is_none() {
        *next_search += 1;
    }
    let pause_until = pause_until(page.rate_limit);
    Ok((Step::Searched { pause_until }, None))
}

#[cfg(test)]
mod tests;
