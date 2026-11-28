//! The GitHub thread's own turns: what it does with a typed search's answer (#465).

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::github::testing::{RATE_LIMIT, Scripted, filter_page, github};
use crate::mirror::HeardRequests;
use crate::status::testing::quiet_board;
use crate::store::testing::OpenStore;
use crate::testing::{cpython, cpython_settings, listed};

/// An hour between each of the thread's questions, so only the search it is handed runs.
const AN_HOUR: Duration = Duration::from_secs(3600);

#[test]
fn an_answer_to_a_search_typed_past_changes_nothing() {
    let db = OpenStore::new("github-thread-stale-search");
    let (to_github, incoming) = mpsc::channel();
    let fetching = Fetching::new(
        mpsc::channel().0,
        HeardRequests::new().0,
        Headless::default(),
    );
    let (refreshes, _done) = Refreshes::new(
        Intervals {
            status: AN_HOUR,
            repository: AN_HOUR,
            on_screen: AN_HOUR,
        },
        incoming,
        fetching,
        listed(vec![cpython_settings()]),
    );
    // A query typed after the one GitHub is answering waits in the thread's queue.
    to_github.send(GitHubMessage::Refresh).unwrap();
    to_github
        .send(GitHubMessage::Search(InboxSearchAsk {
            repository: cpython(),
            text: "newer".to_owned(),
            reply: mpsc::channel().0,
        }))
        .unwrap();
    let (script, _asked) = Scripted::new(vec![filter_page(0, &[], None, RATE_LIMIT)]);
    let github = github(script);
    let mut board = quiet_board();
    let mut syncs = Syncs::default();
    let due = Due::starting_at(Instant::now(), refreshes.intervals);

    searched(Searching {
        board: &mut board,
        store: &db.store,
        github: &github,
        queue: &refreshes,
        syncs: &mut syncs,
        due: &due,
        request: &InboxSearchAsk {
            repository: cpython(),
            text: "older".to_owned(),
            reply: mpsc::channel().0,
        },
    })
    .unwrap();

    let answer = db
        .reader
        .read_snapshot(&cpython(), |snapshot| snapshot.inbox_search("older"))
        .unwrap();
    db.close();
    assert!(answer.is_none(), "the newer query's answer alone counts");
}

#[test]
fn a_saved_filter_s_search_queues_between_turns() {
    let db = OpenStore::new("github-thread-filter-search");
    let (to_github, incoming) = mpsc::channel();
    let fetching = Fetching::new(
        mpsc::channel().0,
        HeardRequests::new().0,
        Headless::default(),
    );
    let (refreshes, _done) = Refreshes::new(
        Intervals {
            status: AN_HOUR,
            repository: AN_HOUR,
            on_screen: AN_HOUR,
        },
        incoming,
        fetching,
        listed(vec![cpython_settings()]),
    );
    let mut syncs = Syncs::default();
    syncs.add(cpython());
    let query: FilterQuery = "label:bug".to_owned().try_into().unwrap();
    to_github
        .send(GitHubMessage::FilterSearch {
            repository: cpython(),
            query: query.clone(),
        })
        .unwrap();
    let (script, _asked) = Scripted::new(Vec::new());
    let github = github(script);
    let due = Due::starting_at(Instant::now(), refreshes.intervals);

    let turn = refreshes.next_turn(&github, &mut syncs, due);

    assert!(matches!(turn, Turn::SyncStep), "{turn:?}");
    let open = syncs.activity().unwrap().open.unwrap();
    assert_eq!(open.repository, cpython());
    assert!(
        (syncs.activity().unwrap().elsewhere).is_empty(),
        "the search belongs to the open Repository, ahead of the syncs"
    );
    db.close();
}

#[test]
fn a_saved_filter_s_search_waits_with_the_syncs_for_a_token() {
    let db = OpenStore::new("github-thread-filter-search-no-token");
    let (to_github, incoming) = mpsc::channel();
    let fetching = Fetching::new(
        mpsc::channel().0,
        HeardRequests::new().0,
        Headless::default(),
    );
    let (refreshes, _done) = Refreshes::new(
        Intervals {
            status: Duration::ZERO,
            repository: AN_HOUR,
            on_screen: AN_HOUR,
        },
        incoming,
        fetching,
        listed(vec![cpython_settings()]),
    );
    let mut syncs = Syncs::default();
    syncs.add(cpython());
    to_github
        .send(GitHubMessage::FilterSearch {
            repository: cpython(),
            query: "label:bug".to_owned().try_into().unwrap(),
        })
        .unwrap();
    let mut board = quiet_board();

    let read = refreshes.wait_for_token(&mut board, &db.store, &mut syncs);

    assert!(matches!(read, Some(TokenRead::Due)));
    let open = syncs.activity().unwrap().open.unwrap();
    assert_eq!(open.repository, cpython());
    db.close();
}
