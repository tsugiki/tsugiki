use super::*;
use crate::github::issues::IssueFacts;
use crate::github::pull_requests::RateLimit;
use crate::github::testing::reply_ok;
use crate::github::{Reply, Unreachable};
use crate::repository::RepositoryName;
use crate::sync::testing::{
    Harness, LOTS, announced, cpython, cross_references, fetched, fetched_nodes, found,
    issue_references_with_more, issues, merged_node, nothing, plain, pr_node, run_to_the_end,
    searches_finding, soon, up_to_the_first_issue,
};
use crate::sync::{RATE_LIMIT_FLOOR, pause_until};
use serde_json::{Value, json};

fn limited_until(from_now: i64) -> Error {
    let resets_at = OffsetDateTime::now_utc().unix_timestamp() + from_now;
    let resets_at = Some(u32::try_from(resets_at).unwrap());
    Error::new(ErrorKind::RateLimited { resets_at }, "limited")
}

/// Whether a sync is still queued after `tries` failures for `problem`.
fn queued_after(tries: u8, problem: &Error) -> bool {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..tries {
        syncs.after_failure(problem, soon());
    }
    syncs.ready_at().is_some()
}

fn broken() -> Error {
    Error::new(ErrorKind::Internal, "GitHub answered with status 500.")
}

#[test]
fn a_failed_step_keeps_its_place_and_runs_again() {
    let harness = Harness::new(
        "failure",
        vec![
            found(&["a"], Some("next"), LOTS),
            Err(Unreachable),
            Err(Unreachable),
            found(&["b"], None, LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);

    let failed = syncs.step(&harness.github, &harness.store).unwrap_err();
    drop(syncs.step(&harness.github, &harness.store).unwrap());

    assert_eq!(failed.kind, ErrorKind::Offline);
    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert_eq!(asked[3].1["after"], "next");
    harness.finish();
}

#[test]
fn a_held_back_step_says_when() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let later = Instant::now() + Duration::from_secs(60);

    syncs.wait_until(later);

    assert_eq!(syncs.ready_at(), Some(later));
}

#[test]
fn nothing_queued_means_nothing_to_do() {
    let harness = Harness::new("empty", vec![]);
    let mut syncs = Syncs::default();

    assert_eq!(syncs.ready_at(), None);
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
    assert_eq!(harness.operations(), [""; 0]);
    harness.finish();
}

#[test]
fn a_repository_already_queued_isnt_queued_twice() {
    let harness = Harness::new(
        "dedupe",
        vec![nothing(LOTS), nothing(LOTS), nothing(LOTS), nothing(LOTS)],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.add(plain(RepositoryName::parse("Python/CPython").unwrap()));

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(harness.operations().len(), 4);
    harness.finish();
}

#[test]
fn a_sync_pauses_only_below_the_floor() {
    let reset_at = OffsetDateTime::UNIX_EPOCH;
    let at_the_floor = RateLimit {
        remaining: RATE_LIMIT_FLOOR,
        reset_at,
    };
    let below = RateLimit {
        remaining: RATE_LIMIT_FLOOR - 1,
        reset_at,
    };
    assert_eq!(pause_until(at_the_floor), None);
    assert_eq!(pause_until(below), Some(reset_at));
}

#[test]
fn the_repository_asked_for_last_runs_first() {
    let harness = Harness::new(
        "queue",
        vec![
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

    drop(run_to_the_end(&mut syncs, &harness));

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    let query = |at: usize| asked[at].1["query"].as_str().unwrap().to_owned();
    assert!(query(3).starts_with("repo:python/peps "), "{}", query(3));
    assert!(query(4).starts_with("repo:python/cpython "), "{}", query(4));
    harness.finish();
}

#[test]
fn a_sync_keeps_its_progress_behind_a_newer_one() {
    let harness = Harness::new(
        "behind",
        vec![
            found(&["a"], Some("next"), LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            found(&[], None, LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    assert_eq!(syncs.step(&harness.github, &harness.store).unwrap(), None);
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));

    // The newer sync's four searches, its moves up to the queue, its two empty window pages and the end, then
    // the older sync's next page.
    for _ in 0..13 {
        let progress = syncs.step(&harness.github, &harness.store).unwrap();
        assert_eq!(announced(progress), None);
    }

    let asked: Vec<(String, Value)> = harness.asked.try_iter().collect();
    assert!(
        asked[5].1["query"]
            .as_str()
            .unwrap()
            .starts_with("repo:python/cpython ")
    );
    assert_eq!(asked[5].1["after"], "next");
    harness.finish();
}

#[test]
fn with_nothing_open_a_refresh_queues_nothing() {
    let harness = Harness::new("refresh-nothing", Vec::new());
    let mut syncs = Syncs::default();
    syncs.refresh_open(&harness.store);
    syncs.refresh_now();
    assert_eq!(syncs.ready_at(), None);
    harness.finish();
}

#[test]
fn the_timer_leaves_a_first_sync_that_gave_up_and_a_click_starts_it_again() {
    let harness = Harness::new("gave-up", Vec::new());
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }
    assert_eq!(syncs.ready_at(), None);

    syncs.refresh_open(&harness.store);
    assert_eq!(syncs.ready_at(), None);

    syncs.refresh_now();
    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn a_click_ends_a_wait_for_the_network_and_keeps_a_wait_for_points() {
    let later = Instant::now() + Duration::from_secs(3600);
    let offline = Error::new(ErrorKind::Offline, "No network.");
    let spent = Error::new(ErrorKind::RateLimited { resets_at: None }, "No points.");
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    syncs.after_failure(&offline, later);
    syncs.refresh_now();
    assert!(syncs.ready_at().unwrap() < later);

    syncs.after_failure(&spent, later);
    syncs.refresh_now();
    assert_eq!(syncs.ready_at(), Some(later));
}

#[test]
fn the_last_sync_says_when_it_finished_and_the_next_asks_from_when_it_started() {
    let harness = Harness::new("finish-time", searches_finding(&[]));
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let began = OffsetDateTime::now_utc() - time::Duration::hours(1);
    syncs.queue.front_mut().unwrap().started_at = began;

    let mut finished = None;
    while syncs.ready_at().is_some() {
        if let Some(Progress::Finished(last_sync)) =
            syncs.step(&harness.github, &harness.store).unwrap()
        {
            finished = last_sync.at;
        }
    }

    let state = harness.store.sync_state(&cpython()).unwrap().unwrap();
    assert_eq!(finished, Some(state.finished_at));
    assert_eq!(state.started_at.unix_timestamp(), began.unix_timestamp());
    assert!(state.finished_at > began + time::Duration::minutes(59));
    harness.finish();
}

#[test]
fn a_refresh_of_the_open_repository_joins_the_queue_once() {
    let harness = Harness::new("refresh-open", searches_finding(&[]));
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    let operations = harness.operations().len();
    assert_eq!(operations, 4, "one sync's four searches");

    syncs.refresh_open(&harness.store);
    syncs.refresh_open(&harness.store);

    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn a_refresh_goes_behind_a_sync_of_another_repository() {
    let peps = RepositoryName::parse("python/peps").unwrap();
    let mut replies = searches_finding(&[]);
    replies.extend(searches_finding(&[]));
    let harness = Harness::new("refresh-behind", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    syncs.add(plain(peps.clone()));
    syncs.add(plain(cpython()));
    syncs.queue.retain(|sync| sync.repository == peps);
    drop(syncs.step(&harness.github, &harness.store).unwrap());

    syncs.refresh_open(&harness.store);

    assert_eq!(syncs.queue.len(), 2);
    assert_eq!(syncs.queue.front().unwrap().repository, peps);
    harness.finish();
}

#[test]
fn only_the_open_repository_says_its_sync_finished() {
    let peps = RepositoryName::parse("python/peps").unwrap();
    let mut replies = searches_finding(&[]);
    replies.extend(searches_finding(&[]));
    let harness = Harness::new("finished", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(peps));
    syncs.add(plain(cpython()));

    let mut finished = Vec::new();
    while syncs.ready_at().is_some() {
        let progress = syncs.step(&harness.github, &harness.store).unwrap();
        if let Some(Progress::Finished(last_sync)) = progress {
            finished.push(last_sync.repository);
        }
    }

    assert_eq!(finished, [cpython()]);
    harness.finish();
}

#[test]
fn a_first_sync_that_gave_way_keeps_the_cross_references_it_had_listed() {
    let mut replies = up_to_the_first_issue();
    replies.push(issue_references_with_more(1, &[3], "older", LOTS));
    replies.push(cross_references(&[2], None, LOTS));
    replies.push(fetched_nodes(&[merged_node(3), merged_node(2)], LOTS));
    let harness = Harness::new("gave-way-listed", replies);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let mut before_it_gave_way = Vec::new();
    while !before_it_gave_way
        .iter()
        .any(|(operation, _)| operation == "FetchIssueReferences")
    {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
        before_it_gave_way.extend(harness.asked.try_iter());
    }

    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }
    assert_eq!(syncs.ready_at(), None);
    syncs.refresh_now();
    drop(run_to_the_end(&mut syncs, &harness));

    let next_page: Vec<Value> = harness
        .asked
        .try_iter()
        .filter(|(operation, _)| operation == "FetchCrossReferences")
        .map(|(_, sent)| sent["before"].clone())
        .collect();
    assert_eq!(next_page, [json!("older")]);
    assert_eq!(harness.numbers(), [1, 2, 3]);
    harness.finish();
}

#[test]
fn a_step_that_keeps_failing_gives_way_to_the_next_sync() {
    let mut syncs = Syncs::default();
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));
    syncs.add(plain(cpython()));

    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }

    assert_eq!(syncs.queue.len(), 1);
    assert_eq!(
        syncs.queue.front().unwrap().repository,
        RepositoryName::parse("python/peps").unwrap()
    );
    assert_eq!(syncs.failures, 0);
}

#[test]
fn failures_with_nothing_queued_change_nothing() {
    let mut syncs = Syncs::default();
    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }
    assert_eq!(syncs.ready_at(), None);
}

#[test]
fn an_outage_shorter_than_the_tries_doesnt_end_a_sync() {
    assert!(queued_after(TRIES - 1, &broken()));
    assert!(!queued_after(TRIES, &broken()));
}

#[test]
fn a_step_that_works_starts_the_count_again() {
    let harness = Harness::new("recovers", vec![nothing(LOTS)]);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES - 1 {
        syncs.after_failure(&broken(), soon());
    }

    drop(syncs.step(&harness.github, &harness.store).unwrap());
    syncs.after_failure(&broken(), soon());

    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn asking_again_starts_the_count_again() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..TRIES - 1 {
        syncs.after_failure(&broken(), soon());
    }

    syncs.add(plain(cpython()));
    syncs.after_failure(&broken(), soon());

    assert_eq!(syncs.queue.len(), 1);
}

#[test]
fn no_network_never_ends_a_sync() {
    let offline = Error::new(ErrorKind::Offline, "no network");
    assert!(queued_after(u8::MAX, &offline));
}

#[test]
fn a_rate_limit_never_ends_a_sync() {
    let limited = Error::new(ErrorKind::RateLimited { resets_at: None }, "limited");
    assert!(queued_after(u8::MAX, &limited));
}

#[test]
fn a_failure_waits_for_the_next_refresh() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let next_refresh = soon();

    syncs.after_failure(&broken(), next_refresh);

    assert_eq!(syncs.ready_at(), Some(next_refresh));
}

#[test]
fn a_rate_limit_waits_for_its_reset() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    syncs.after_failure(&limited_until(3000), soon());

    let wait = syncs.ready_at().unwrap() - Instant::now();
    assert!(wait > Duration::from_secs(2900), "{wait:?}");
    assert!(wait <= Duration::from_secs(3001), "{wait:?}");
}

#[test]
fn a_rate_limit_whose_reset_has_passed_still_waits_for_the_next_refresh() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    let next_refresh = soon();

    syncs.after_failure(&limited_until(-10), next_refresh);

    assert_eq!(syncs.ready_at(), Some(next_refresh));
}

#[test]
fn a_save_that_fails_is_the_steps_failure() {
    let harness = Harness::new(
        "save-fails",
        vec![
            found(&["a"], None, LOTS),
            nothing(LOTS),
            nothing(LOTS),
            nothing(LOTS),
            fetched(&[1], LOTS),
        ],
    );
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    for _ in 0..5 {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }
    let stopped = Store::stopped();

    let failed = syncs.step(&harness.github, &stopped).unwrap_err();
    stopped.close();

    assert_eq!(failed.kind, ErrorKind::Internal);
    harness.finish();
}

#[test]
fn a_pause_is_as_long_as_the_reset_is_away_within_reason() {
    let now = OffsetDateTime::UNIX_EPOCH;
    let minute = time::Duration::minutes(1);

    assert_eq!(pause_for(now + minute, now), Duration::from_secs(60));
    assert_eq!(pause_for(now - minute, now), Duration::ZERO);
    assert_eq!(pause_for(now + minute * 600, now), LONGEST_PAUSE);
}

mod activity {
    use super::*;
    use crate::status::{SyncActivity, SyncPart};
    use crate::sync::testing::{FEW, empty_window_page, hours_ago, window_issue, window_page};

    fn part_and_pages(syncs: &Syncs) -> (SyncPart, u32) {
        let activity = syncs.activity().unwrap();
        (activity.part, activity.pages)
    }

    #[test]
    fn an_empty_queue_is_doing_nothing() {
        assert_eq!(Syncs::default().activity(), None);
    }

    #[test]
    fn a_first_sync_says_its_part_and_the_pages_it_has_read() {
        let mut replies = searches_finding(&[]);
        replies.push(window_page(
            &[window_issue(1, &hours_ago(1))],
            Some("more"),
            LOTS,
        ));
        replies.push(empty_window_page());
        replies.push(empty_window_page());
        let harness = Harness::with_window("activity-parts", replies);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        let mut seen = Vec::new();
        while syncs.ready_at().is_some() {
            drop(syncs.step(&harness.github, &harness.store).unwrap());
            if syncs.ready_at().is_some() {
                seen.push(part_and_pages(&syncs));
            }
        }

        seen.dedup();
        assert_eq!(
            seen,
            [
                (SyncPart::YourWork, 1),
                (SyncPart::YourWork, 2),
                (SyncPart::YourWork, 3),
                (SyncPart::YourWork, 4),
                (SyncPart::Window, 4),
                (SyncPart::Window, 5),
                (SyncPart::Window, 6),
                (SyncPart::Window, 7),
            ]
        );
        harness.finish();
    }

    #[test]
    fn a_sync_below_the_floor_says_when_it_goes_on() {
        let harness = Harness::new("activity-floor", vec![nothing(FEW)]);
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        drop(syncs.step(&harness.github, &harness.store).unwrap());

        let Some(SyncActivity { paused_until, .. }) = syncs.activity() else {
            panic!("the sync is under way");
        };
        let wait = paused_until.unwrap() - time::OffsetDateTime::now_utc();
        assert!(wait > time::Duration::minutes(30));
        harness.finish();
    }

    #[test]
    fn a_wait_for_the_network_isnt_a_wait_for_points() {
        let mut syncs = Syncs::default();
        syncs.add(plain(cpython()));

        syncs.wait_until(Instant::now() + Duration::from_secs(60));

        assert_eq!(syncs.activity().unwrap().paused_until, None);
    }
}

/// Syncs and a harness after a first sync that found nothing, with `replies` for what comes next.
fn after_a_first_sync(name: &str, replies: Vec<Result<Reply, Unreachable>>) -> (Syncs, Harness) {
    let mut all = searches_finding(&[]);
    all.extend(replies);
    let harness = Harness::new(name, all);
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    drop(run_to_the_end(&mut syncs, &harness));
    drop(harness.operations());
    (syncs, harness)
}

#[test]
fn opening_an_issue_after_a_first_sync_reads_its_whole_tree_once() {
    let (mut syncs, harness) = after_a_first_sync(
        "tree-open",
        vec![
            cross_references(&[20], Some("older"), LOTS),
            cross_references(&[21], None, LOTS),
            fetched_nodes(&[merged_node(20), merged_node(21)], LOTS),
            issues(&[1], LOTS),
        ],
    );
    let last_sync = harness.store.sync_state(&cpython()).unwrap();

    syncs.open_issue(&cpython(), 7);
    syncs.open_issue(&cpython(), 7);

    assert_eq!(syncs.activity().unwrap().part, SyncPart::Trees);
    assert_eq!(run_to_the_end(&mut syncs, &harness), [cpython(), cpython()]);
    assert_eq!(harness.numbers(), [20, 21]);
    assert_eq!(harness.store.sync_state(&cpython()).unwrap(), last_sync);
    // A view on screen asks about once a minute, and by now the tree has been read.
    syncs.open_issue(&cpython(), 7);
    assert_eq!(syncs.ready_at(), None);
    assert_eq!(
        harness.operations(),
        [
            "FetchCrossReferences",
            "FetchCrossReferences",
            "FetchTreePullRequests",
            "FetchIssues"
        ]
    );
    harness.finish();
}

#[test]
fn a_click_on_refresh_waits_behind_a_tree_and_still_runs() {
    let (mut syncs, harness) =
        after_a_first_sync("tree-click", vec![cross_references(&[], None, LOTS)]);

    syncs.open_issue(&cpython(), 7);
    syncs.refresh_now();

    assert_eq!(syncs.activity().unwrap().part, SyncPart::Trees);
    while !syncs.trees.is_empty() {
        drop(syncs.step(&harness.github, &harness.store).unwrap());
    }
    assert_eq!(syncs.activity().unwrap().part, SyncPart::Refresh);
    assert_eq!(syncs.queue.len(), 1);
    harness.finish();
}

#[test]
fn opening_an_issue_with_no_queue_to_join_reads_its_tree_alone() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));

    syncs.open_issue(&cpython(), 7);

    assert_eq!(syncs.trees.len(), 1);
    assert_eq!(syncs.queue.len(), 1);
}

#[test]
fn only_the_open_repository_reads_an_opened_tree() {
    let mut syncs = Syncs::default();
    syncs.open_issue(&cpython(), 7);
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));
    syncs.open_issue(&cpython(), 7);

    assert!(syncs.trees.is_empty());
}

#[test]
fn coming_back_to_a_repository_reads_an_opened_tree_again() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.open_issue(&cpython(), 7);
    syncs.add(plain(RepositoryName::parse("python/peps").unwrap()));
    syncs.add(plain(cpython()));

    syncs.open_issue(&cpython(), 7);

    assert_eq!(syncs.trees.len(), 1);
}

#[test]
fn another_repository_doesnt_wait_behind_an_opened_tree() {
    let peps = RepositoryName::parse("python/peps").unwrap();
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.open_issue(&cpython(), 7);

    syncs.add(plain(peps.clone()));

    assert_eq!(syncs.activity().unwrap().repository, peps);
}

fn facts_of_issue(number: u32) -> Vec<IssueFacts> {
    vec![IssueFacts {
        number,
        title: format!("Issue {number}"),
        author: None,
        assignees: Vec::new(),
        updated_at: OffsetDateTime::now_utc(),
    }]
}

fn refused() -> Result<Reply, Unreachable> {
    reply_ok(r#"{"data":null,"errors":[{"type":"FORBIDDEN","message":"No."}]}"#)
}

#[test]
fn a_tree_github_refuses_leaves_at_once_and_can_be_asked_for_again() {
    let (mut syncs, harness) = after_a_first_sync("tree-refused", vec![refused()]);
    syncs.open_issue(&cpython(), 7);
    syncs.refresh_now();

    assert!(matches!(
        syncs.step(&harness.github, &harness.store),
        Ok(None)
    ));

    assert!(syncs.trees.is_empty());
    assert_eq!(syncs.activity().unwrap().part, SyncPart::Refresh);
    syncs.open_issue(&cpython(), 7);
    assert_eq!(syncs.trees.len(), 1);
    harness.finish();
}

#[test]
fn a_tree_waits_while_github_cant_be_reached() {
    let (mut syncs, harness) = after_a_first_sync("tree-offline", vec![Err(Unreachable)]);
    syncs.open_issue(&cpython(), 7);

    let problem = syncs.step(&harness.github, &harness.store).unwrap_err();

    assert_eq!(problem.kind, ErrorKind::Offline);
    assert_eq!(syncs.trees.len(), 1);
    harness.finish();
}

#[test]
fn a_tree_brings_only_the_issues_its_prs_name_that_tsugiki_lacks() {
    let named = |number: u32, title: &str| {
        pr_node(
            number,
            &json!({ "title": title, "state": "MERGED", "mergeCommit": { "oid": format!("{number:040x}") } }),
        )
    };
    let (mut syncs, harness) = after_a_first_sync(
        "tree-named",
        vec![
            cross_references(&[20, 21], None, LOTS),
            fetched_nodes(
                &[named(20, "gh-5: Fix it"), named(21, "gh-7: Fix it")],
                LOTS,
            ),
            issues(&[5], LOTS),
        ],
    );
    harness
        .store
        .save_fetched_issues(&cpython(), facts_of_issue(7), Vec::new())
        .unwrap();
    syncs.open_issue(&cpython(), 7);

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        [
            "FetchCrossReferences",
            "FetchTreePullRequests",
            "FetchIssues"
        ]
    );
    let held = harness.store.issues(&cpython()).unwrap();
    let mut numbers: Vec<u32> = held.iter().map(|issue| issue.number).collect();
    numbers.sort_unstable();
    assert_eq!(numbers, [5, 7]);
    harness.finish();
}

#[test]
fn a_tree_whose_prs_name_issues_tsugiki_has_asks_for_no_more() {
    let (mut syncs, harness) = after_a_first_sync(
        "tree-named-held",
        vec![
            cross_references(&[20], None, LOTS),
            fetched_nodes(&[pr_node(20, &json!({ "title": "gh-7: Fix it" }))], LOTS),
        ],
    );
    harness
        .store
        .save_fetched_issues(&cpython(), facts_of_issue(7), Vec::new())
        .unwrap();
    syncs.open_issue(&cpython(), 7);

    drop(run_to_the_end(&mut syncs, &harness));

    assert_eq!(
        harness.operations(),
        ["FetchCrossReferences", "FetchTreePullRequests"]
    );
    harness.finish();
}

#[test]
fn a_tree_that_keeps_failing_gives_way_to_the_sync_behind_it() {
    let mut syncs = Syncs::default();
    syncs.add(plain(cpython()));
    syncs.open_issue(&cpython(), 7);

    for _ in 0..TRIES {
        syncs.after_failure(&broken(), soon());
    }

    assert!(syncs.trees.is_empty());
    assert_eq!(syncs.queue.len(), 1);
    syncs.open_issue(&cpython(), 7);
    assert_eq!(syncs.trees.len(), 1);
}
