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
        .send(GitHubMessage::Search {
            repository: cpython(),
            text: "newer".to_owned(),
        })
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
        request: SearchRequest {
            repository: cpython(),
            text: "older".to_owned(),
        },
    });

    let answer = db
        .reader
        .read_snapshot(&cpython(), |snapshot| snapshot.inbox_search("older"))
        .unwrap();
    db.close();
    assert!(answer.is_none(), "the newer query's answer alone counts");
}
