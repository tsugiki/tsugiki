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
mod tests {
    use crate::sync::testing::{
        FEW, Harness, LOTS, cpython, fetched, found, nothing, plain, run_to_the_end,
    };
    use crate::sync::{Syncs, WORK_SEARCHES};
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    #[test]
    fn a_sync_searches_in_order_then_stores_what_it_found() {
        let harness = Harness::new(
            "order",
            vec![
                found(&["a"], None, LOTS),
                found(&["b"], None, LOTS),
                found(&["c"], None, LOTS),
                found(&["d"], None, LOTS),
                fetched(&[1, 2, 3, 4], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        assert_eq!(
            harness.operations(),
            [
                "SearchReviewRequests",
                "SearchReviewedPrs",
                "SearchOwnPrs",
                "SearchAssignedPrs",
                "FetchPullRequests",
            ]
        );
        assert_eq!(harness.numbers(), [1, 2, 3, 4]);
        assert_eq!(changed, [cpython()]);
        harness.finish();
    }

    #[test]
    fn a_search_follows_its_pages_before_the_next_search() {
        let harness = Harness::new(
            "pages",
            vec![
                found(&["a"], Some("next"), LOTS),
                found(&["b"], None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1, 2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[0].1["after"], Value::Null);
        assert_eq!(asked[1].0, "SearchReviewRequests");
        assert_eq!(asked[1].1["after"], "next");
        assert_eq!(asked[2].0, "SearchReviewedPrs");
        assert_eq!(asked[2].1["after"], Value::Null);
        assert_eq!(asked[5].1["ids"], json!(["a", "b"]));
        harness.finish();
    }

    #[test]
    fn a_search_below_the_floor_holds_the_next_step_back() {
        let harness = Harness::new("floor-search", vec![nothing(FEW)]);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        drop(syncs.step(&harness.github, &harness.store).unwrap());

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }

    #[test]
    fn every_search_finds_open_prs() {
        for search in WORK_SEARCHES {
            assert!(
                search.qualifiers.starts_with("is:pr is:open "),
                "{search:?}"
            );
        }
    }
}
