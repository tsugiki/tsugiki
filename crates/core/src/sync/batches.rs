//! Fetching the PRs a sync found, a batch at a time and each once.

use std::collections::BTreeSet;

use super::{Batches, FETCH_BATCH, Link, Moved, Resumable, Step, pause_until};
use crate::error::Error;

pub(super) fn fetch(
    link: &Link<'_>,
    batches: &mut Batches,
    fetched: &mut BTreeSet<u32>,
) -> Result<Moved, Error> {
    let Batches { ids, why, leads } = batches;
    if ids.is_empty() {
        leads.settle_origins(link, fetched)?;
        let next = Resumable::FetchingOrigins {
            why: *why,
            leads: std::mem::take(leads),
        }
        .into();
        return Ok((Step::Moved, Some(next)));
    }
    let batch = link.github.fetch_pull_requests(*why, ids, FETCH_BATCH)?;
    link.store
        .save_pull_requests(link.repository, batch.pull_requests.clone())?;
    for pull_request in &batch.pull_requests {
        leads.learn_from(pull_request, link.repository);
        fetched.extend([pull_request.number]);
    }
    let touched = batch.pull_requests.iter().map(|pr| pr.number).collect();
    drop(ids.drain(..batch.asked_for));
    let pause_until = pause_until(batch.rate_limit);
    let step = Step::Saved {
        touched,
        pause_until,
    };
    Ok((step, None))
}

#[cfg(test)]
mod tests {
    use crate::github::Unreachable;
    use crate::sync::Syncs;
    use crate::sync::testing::{
        FEW, Harness, LOTS, cpython, fetched, found, nothing, plain, run_to_the_end,
    };
    use serde_json::{Value, json};
    use std::time::{Duration, Instant};

    #[test]
    fn a_pr_two_searches_found_is_fetched_once() {
        let harness = Harness::new(
            "twice",
            vec![
                found(&["a"], None, LOTS),
                found(&["a"], None, LOTS),
                found(&["a", "b"], None, LOTS),
                nothing(LOTS),
                fetched(&[1, 2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[4].1["ids"], json!(["a", "b"]));
        harness.finish();
    }

    #[test]
    fn a_retry_at_half_size_leaves_the_rest_for_the_next_batch() {
        let ids: Vec<String> = (0..4).map(|n| format!("id{n}")).collect();
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let harness = Harness::new(
            "half",
            vec![
                found(&ids, None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                Err(Unreachable),
                fetched(&[1, 2], LOTS),
                fetched(&[3, 4], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let changed = run_to_the_end(&mut syncs, &harness);

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[5].1["ids"], json!(["id0", "id1"]));
        assert_eq!(asked[6].1["ids"], json!(["id2", "id3"]));
        assert_eq!(harness.numbers(), [1, 2, 3, 4]);
        assert_eq!(changed, [cpython(), cpython()]);
        harness.finish();
    }

    #[test]
    fn a_fetch_below_the_floor_holds_the_next_step_back() {
        let harness = Harness::new(
            "floor-fetch",
            vec![
                found(&["a"], None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1], FEW),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));
        let before = Instant::now();

        for _ in 0..7 {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
        }

        let ready = syncs.ready_at().unwrap();
        assert!(ready > before + Duration::from_secs(3000), "{ready:?}");
        assert_eq!(harness.numbers(), [1]);
        harness.finish();
    }

    #[test]
    fn a_sync_asks_for_pages_of_50_and_batches_of_25() {
        let names: Vec<String> = (0..30).map(|n| format!("id{n}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let harness = Harness::new(
            "sizes",
            vec![
                found(&names, None, LOTS),
                nothing(LOTS),
                nothing(LOTS),
                nothing(LOTS),
                fetched(&[1], LOTS),
                fetched(&[2], LOTS),
            ],
        );
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(run_to_the_end(&mut syncs, &harness));

        let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
        assert_eq!(asked[0].1["first"], 50);
        assert_eq!(
            asked[0].1["query"],
            "repo:python/cpython is:pr is:open user-review-requested:@me"
        );
        assert_eq!(asked[4].1["ids"].as_array().unwrap().len(), 25);
        assert_eq!(asked[5].1["ids"].as_array().unwrap().len(), 5);
        harness.finish();
    }
}
