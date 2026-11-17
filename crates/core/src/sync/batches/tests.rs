use crate::github::Unreachable;
use crate::sync::Syncs;
use crate::sync::testing::{
    FEW, Harness, RATE_LIMIT, cpython, fetched, nothing, plain, run_to_the_end, search_page,
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[test]
fn a_pr_two_searches_found_is_fetched_once() {
    let harness = Harness::new(
        "twice",
        vec![
            search_page(&["a"], None, RATE_LIMIT),
            search_page(&["a"], None, RATE_LIMIT),
            search_page(&["a", "b"], None, RATE_LIMIT),
            nothing(RATE_LIMIT),
            fetched(&[1, 2], RATE_LIMIT),
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
            search_page(&ids, None, RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            Err(Unreachable::Offline),
            fetched(&[1, 2], RATE_LIMIT),
            fetched(&[3, 4], RATE_LIMIT),
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
            search_page(&["a"], None, RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            fetched(&[1], FEW),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let before = Instant::now();

    for _ in 0..7 {
        drop(syncs.step(&harness.github, &harness.db.store).unwrap());
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
            search_page(&names, None, RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            nothing(RATE_LIMIT),
            fetched(&[1], RATE_LIMIT),
            fetched(&[2], RATE_LIMIT),
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
