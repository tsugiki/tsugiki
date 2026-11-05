//! Reading each Issue's cross-references, which list the PRs of its tree (ADR 0011).

use std::collections::BTreeSet;

use super::{Batches, Leads, Link, Moved, Reading, Resumable, SEARCH_PAGE, Step, pause_until};
use crate::error::Error;
use crate::github::issues::ListedPr;
use crate::github::pull_requests::{NodeId, PageRequest, PrFetch};

pub(super) fn read_cross_references(
    link: &Link<'_>,
    reading: &mut Reading,
    fetched: &BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Reading {
        issues,
        cursor,
        listed,
    } = reading;
    let Some(issue) = issues.first().copied() else {
        let frozen = link.store.frozen_pull_requests(link.repository)?;
        let frozen: BTreeSet<u32> = frozen.iter().map(|pr| pr.number).collect();
        let leads = leads_of_those_held(link, listed, fetched)?;
        let next = Resumable::Fetching(Batches {
            ids: not_yet_fetched(std::mem::take(listed), fetched, &frozen),
            why: PrFetch::Tree,
            leads,
        })
        .into();
        return Ok((Step::Moved, Some(next)));
    };
    let asked = PageRequest {
        size: SEARCH_PAGE,
        after: cursor.as_deref(),
    };
    let page = link
        .github
        .fetch_cross_references(link.repository, issue, asked)?;
    listed.extend(page.listed);
    *cursor = page.next;
    if cursor.is_none() {
        drop(issues.drain(..1));
    }
    let pause_until = pause_until(page.rate_limit);
    Ok((Step::Searched { pause_until }, None))
}

/// The leads of the PRs in `listed` that this sync has already saved. The window saves PRs without following
/// them, because few are in any tree, so a Backport it brought only gets its Origin PR once a tree lists it.
fn leads_of_those_held(
    link: &Link<'_>,
    listed: &[ListedPr],
    fetched: &BTreeSet<u32>,
) -> Result<Leads, Error> {
    let numbers = listed.iter().map(|pr| pr.number);
    let held: BTreeSet<u32> = numbers.filter(|number| fetched.contains(number)).collect();
    let mut leads = Leads::default();
    if !held.is_empty() {
        let stored = link.store.pull_requests(link.repository)?;
        for pull_request in stored.iter().filter(|pr| held.contains(&pr.number)) {
            leads.learn_from(pull_request, link.repository);
        }
    }
    Ok(leads)
}

/// The IDs of the PRs in `listed` to fetch, each once. A PR Tsugiki keeps as merged or closed stays frozen,
/// and an open one is fetched again, because it may not be open any more (ADR 0011). The link rules read what
/// comes back when a screen asks, so a PR that only mentions an Issue stays out of its tree.
fn not_yet_fetched(
    listed: Vec<ListedPr>,
    fetched: &BTreeSet<u32>,
    frozen: &BTreeSet<u32>,
) -> Vec<NodeId> {
    let mut seen = BTreeSet::new();
    listed
        .into_iter()
        .filter(|pr| {
            !fetched.contains(&pr.number) && !frozen.contains(&pr.number) && seen.insert(pr.number)
        })
        .map(|pr| pr.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::sync::Syncs;
    use crate::sync::testing::{
        FEW, Harness, LOTS, cpython, cross_references, fetched_nodes, issues, merged_node, plain,
        pr_node, run_to_the_end, searches_finding, up_to_the_first_issue,
    };
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    #[test]
    fn a_sync_reads_each_shown_issues_cross_references_and_fetches_the_prs_they_list() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(2), merged_node(3)], LOTS));
        let harness = Harness::new("tree", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        assert_eq!(
            harness.operations()[4..],
            [
                "FetchPullRequests",
                "FetchIssues",
                "FetchCrossReferences",
                "FetchTreePullRequests",
            ]
        );
        assert_eq!(harness.numbers(), [1, 2, 3]);
        harness.finish();
    }

    #[test]
    fn a_sync_follows_an_issues_cross_references_page_by_page() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[3], Some("older"), LOTS));
        replies.push(cross_references(&[2], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(3), merged_node(2)], LOTS));
        let harness = Harness::new("tree-pages", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let befores: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchCrossReferences")
            .map(|(_, sent)| sent["before"].clone())
            .collect();
        assert_eq!(befores, [Value::Null, json!("older")]);
        assert_eq!(harness.numbers(), [1, 2, 3]);
        harness.finish();
    }

    #[test]
    fn a_sync_reads_the_cross_references_of_each_issue_and_fetches_a_shared_pr_once() {
        let mut replies = searches_finding(&["a"]);
        replies.push(fetched_nodes(
            &[pr_node(1, &json!({ "title": "gh-1, gh-2: Fix both" }))],
            LOTS,
        ));
        replies.push(issues(&[1, 2], LOTS));
        replies.push(cross_references(&[3, 1], None, LOTS));
        replies.push(cross_references(&[3], None, LOTS));
        replies.push(fetched_nodes(&[merged_node(3)], LOTS));
        let harness = Harness::new("tree-two", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let tree_ids: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchTreePullRequests")
            .map(|(_, sent)| sent["ids"].clone())
            .collect();
        assert_eq!(tree_ids, [json!(["id3"])]);
        harness.finish();
    }

    #[test]
    fn a_merged_pr_is_fetched_once_across_syncs_and_an_open_one_again() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(
            &[merged_node(2), pr_node(3, &json!({}))],
            LOTS,
        ));
        replies.extend(up_to_the_first_issue());
        replies.push(cross_references(&[2, 3], None, LOTS));
        replies.push(fetched_nodes(&[pr_node(3, &json!({}))], LOTS));
        let harness = Harness::new("frozen", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));
        drop(harness.operations());

        harness.forget_the_last_sync();
        syncs.add(plain(cpython()));
        drop(run_to_the_end(&mut syncs, &harness));

        let tree_ids: Vec<Value> = harness
            .asked
            .try_iter()
            .filter(|(operation, _)| operation == "FetchTreePullRequests")
            .map(|(_, sent)| sent["ids"].clone())
            .collect();
        assert_eq!(tree_ids, [json!(["id3"])]);
        harness.finish();
    }

    #[test]
    fn a_tree_read_below_the_floor_holds_the_next_step_back() {
        let mut replies = up_to_the_first_issue();
        replies.push(cross_references(&[], Some("older"), FEW));
        let harness = Harness::new("tree-floor", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        // Four searches, two moves, the PR, two moves, the Issue, a move, two empty window pages, a
        // move and the first page.
        for _ in 0..15 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        harness.finish();
    }
}
