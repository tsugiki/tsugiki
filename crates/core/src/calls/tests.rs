use std::sync::mpsc::Receiver;
use std::time::Duration;

use serde_json::json;
use tsugiki_test_git::git;

use super::*;
use crate::definitions::Sources;
use crate::diff::testing::Mirrored;
use crate::facts::pull_requests::PrHead;
use crate::fetching::testing::handle;
use crate::fetching::{Fetched, SessionMessage};
use crate::mirror::HeardRequests;
use crate::workbench::pr_files::testing::World;

/// PR 7's world, and the queues and language servers its calls ask, which only listen.
struct Calls {
    world: World,
    fetching: Fetching,
    session: Receiver<SessionMessage>,
    heard: HeardRequests,
    definitions: Definitions,
}

impl Calls {
    fn new(name: &str) -> Self {
        let (fetching, session, heard) = handle();
        let definitions = Definitions::start(Sources {
            checkout: Box::new(|_| None),
            servers: Box::new(|_| None),
            idle: Box::new(|| Duration::from_secs(3600)),
            changed: Box::new(|_| {}),
        });
        Self {
            world: World::new(&format!("calls-{name}")),
            fetching,
            session,
            heard,
            definitions,
        }
    }

    fn work(&self) -> PullRequestWork<'_> {
        PullRequestWork {
            sources: self.world.sources(),
            fetching: &self.fetching,
            definitions: &self.definitions,
        }
    }

    /// What PR 7's Files tab shows.
    fn pr_files(&self) -> Option<PrFiles> {
        self.work().pr_files(&self.world.repository(), 7).unwrap()
    }

    /// The PRs whose Conversations the session was asked to read.
    fn conversations_read(&self) -> Vec<u32> {
        (self.session.try_iter())
            .filter_map(|message| match message {
                SessionMessage::Fetch(Fetched::PullRequest(item)) => Some(item.number),
                SessionMessage::Fetch(Fetched::Issue(_))
                | SessionMessage::Stop
                | SessionMessage::Check { .. }
                | SessionMessage::Sync { .. }
                | SessionMessage::Refresh
                | SessionMessage::SettingsChanged(..)
                | SessionMessage::Mark(_)
                | SessionMessage::Patched(..) => None,
            })
            .collect()
    }

    fn finish(self) {
        self.world.finish();
    }
}

/// PR 7's head at a commit no mirror holds.
fn elsewhere() -> PrHead {
    PrHead {
        number: 7,
        commit: CommitSha::parse(&"3".repeat(40)).unwrap(),
        target_branch: "main".to_owned(),
    }
}

#[test]
fn the_files_tab_of_a_pr_whose_head_tsugiki_doesnt_know_reads_its_conversation() {
    let calls = Calls::new("no-head");
    let mirrored = Mirrored::new("calls-no-head");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(None, "main");

    let files = calls.pr_files();

    assert!(matches!(files, Some(PrFiles::Fetching { .. })), "{files:?}");
    assert_eq!(
        serde_json::to_value(files).unwrap(),
        json!({ "state": "fetching" })
    );
    assert_eq!(calls.conversations_read(), [7]);
    assert_eq!(calls.heard.asked(), ["python/cpython awaits #7"]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_head_the_mirror_lacks_fetches_it_first() {
    let calls = Calls::new("missing");
    let mirrored = Mirrored::new("calls-missing");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(Some(elsewhere().commit), "main");

    let files = calls.pr_files();

    assert!(matches!(files, Some(PrFiles::Fetching { .. })), "{files:?}");
    assert_eq!(
        calls.heard.firsts(),
        [(calls.world.repository(), elsewhere())]
    );
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_head_the_mirror_holds_shows_its_diff_and_moves_the_lookup_checkout_there() {
    let calls = Calls::new("held");
    let mirrored = Mirrored::new("calls-held");
    let whole = mirrored.read();
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(Some(mirrored.head.clone()), "main");

    let files = calls.pr_files();

    let at = LookupAt {
        base: whole.base.clone(),
        head: mirrored.head.clone(),
    };
    assert_eq!(calls.heard.look_ups(), [(calls.world.repository(), at)]);
    assert_eq!(calls.heard.asked(), ["python/cpython opened #7"]);
    let shown = serde_json::to_value(&files).unwrap();
    assert_eq!(
        (&shown["state"], &shown["head"], &shown["threads"]),
        (
            &json!("ready"),
            &json!(mirrored.head.as_str()),
            &json!({ "kind": "shown", "threads": [] })
        ),
        "the diff's fields sit beside the state, as the window reads them"
    );
    calls.finish();
}

#[test]
fn the_files_tab_of_a_moved_head_shows_the_diff_before_and_fetches_the_newer_first() {
    let calls = Calls::new("moved");
    let mirrored = Mirrored::new("calls-moved");
    let older = mirrored.head.clone();
    let _ = git(
        &mirrored.mirror,
        &["update-ref", "refs/tsugiki/pull/7/head", older.as_str()],
    );
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(Some(elsewhere().commit), "main");

    let files = calls.pr_files();

    let Some(PrFiles::Ready { diff, threads }) = files else {
        panic!("no diff in {files:?}");
    };
    assert_eq!(diff.head, older);
    assert!(
        matches!(&threads, DiffThreads::HeldBack { newer, count: 0, .. } if *newer == elsewhere().commit),
        "{threads:?}"
    );
    assert_eq!(
        calls.heard.firsts(),
        [(calls.world.repository(), elsewhere())]
    );
    calls.finish();
}

#[test]
fn before_its_mirror_is_cloned_the_files_tab_shows_the_clone_it_queued() {
    let calls = Calls::new("cloning");
    calls.world.has_pr(None, "main");

    let files = calls.pr_files();

    assert!(matches!(files, Some(PrFiles::Cloning { .. })), "{files:?}");
    assert_eq!(
        serde_json::to_value(files).unwrap(),
        json!({ "state": "cloning" })
    );
    assert_eq!(calls.heard.clones(), [calls.world.repository()]);
    calls.finish();
}

#[test]
fn a_clone_that_waits_for_the_user_asks_and_queues_nothing() {
    let calls = Calls::new("asking");

    let files = (calls.work()).answer(&calls.world.repository(), 7, FilesRead::Asking);

    assert_eq!(files, PrFiles::Asking);
    assert_eq!(calls.heard.clones(), []);
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_pr_tsugiki_doesnt_hold_shows_nothing_and_asks_nothing() {
    let calls = Calls::new("none");

    assert_eq!(calls.pr_files(), None);
    assert_eq!(calls.heard.clones(), []);
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    calls.finish();
}

#[test]
fn opening_a_pr_reads_its_conversation_and_fetches_a_head_the_mirror_lacks_first() {
    let calls = Calls::new("open-missing");
    let mirrored = Mirrored::new("calls-open-missing");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(Some(elsewhere().commit), "main");

    calls
        .work()
        .open_pull_request(&calls.world.repository(), 7)
        .unwrap();

    assert_eq!(calls.conversations_read(), [7]);
    assert_eq!(
        calls.heard.firsts(),
        [(calls.world.repository(), elsewhere())]
    );
    calls.finish();
}

#[test]
fn opening_a_pr_whose_head_tsugiki_doesnt_know_reads_its_conversation_once() {
    let calls = Calls::new("open-no-head");
    let mirrored = Mirrored::new("calls-open-no-head");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(None, "main");

    calls
        .work()
        .open_pull_request(&calls.world.repository(), 7)
        .unwrap();

    assert_eq!(calls.conversations_read(), [7]);
    assert_eq!(calls.heard.asked(), ["python/cpython awaits #7"]);
    calls.finish();
}
