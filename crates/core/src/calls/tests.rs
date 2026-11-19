use std::fs;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use serde_json::json;
use tsugiki_test_git::git;

use super::*;
use crate::definitions::Sources;
use crate::diff::testing::Mirrored;
use crate::facts::pull_requests::PrHead;
use crate::fetching::testing::handle_with_headless;
use crate::fetching::{Fetched, Headless, Item, SessionMessage};
use crate::latest::{self, LatestWriter};
use crate::mirror::HeardRequests;
use crate::repository_settings::RepositorySettings;
use crate::settings::{LoadedSettings, Settings, SettingsFiles};
use crate::store::testing::OpenStore;
use crate::workbench::pr_files::testing::World;

/// PR 7's world, settings that list its Repository, and the queues and language servers its calls ask,
/// which only listen, with the writer of what the settings say and the PRs whose Conversation named no
/// head.
struct Setup {
    world: World,
    settings: LatestWriter<SettingsView>,
    view: LatestReader<SettingsView>,
    headless: Headless,
    fetching: Fetching,
    session: Receiver<SessionMessage>,
    heard: HeardRequests,
    definitions: Definitions,
}

impl Setup {
    fn new(name: &str) -> Self {
        let (fetching, session, heard, headless) = handle_with_headless();
        let definitions = no_servers();
        let world = World::new(&format!("calls-{name}"));
        let (settings, view) = latest::channel(listing(vec![world.view.settings()]));
        Self {
            world,
            settings,
            view,
            headless,
            fetching,
            session,
            heard,
            definitions,
        }
    }

    fn work(&self) -> PullRequestWork<'_> {
        PullRequestWork {
            sources: Ok(self.world.sources()),
            settings: self.view.clone(),
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

/// The language servers' thread, with no server to start and no Lookup checkout.
fn no_servers() -> Definitions {
    Definitions::start(Sources {
        checkout: Box::new(|_| None),
        servers: Box::new(|_| None),
        idle: Box::new(|| Duration::from_secs(3600)),
        changed: Box::new(|_| {}),
    })
}

/// What `messages` asked the session to sync and which settings they handed it, in words, in order.
fn synced_and_saved(messages: &Receiver<SessionMessage>) -> Vec<String> {
    (messages.try_iter())
        .filter_map(|message| match message {
            SessionMessage::Sync { repository } => Some(format!("sync {}", repository.name)),
            SessionMessage::SettingsChanged(repository, by) => {
                let filters = repository.filters.len();
                Some(format!(
                    "{} saved by {by:?}, {filters} Filter",
                    repository.name
                ))
            }
            SessionMessage::Fetch(_)
            | SessionMessage::Stop
            | SessionMessage::Check { .. }
            | SessionMessage::Refresh
            | SessionMessage::Mark(_)
            | SessionMessage::Patched(..) => None,
        })
        .collect()
}

#[test]
fn the_handle_syncs_the_open_repository_and_clones_its_mirror_first_and_hands_the_session_each_save()
 {
    let root = crate::testing::temp_path("calls-launch");
    fs::create_dir_all(&root).unwrap();
    let folders = crate::testing::one_folder(&root);
    let two = "[[repositories]]\nname = \"a/b\"\n\n[[repositories]]\nname = \"c/d\"\n";
    fs::write(folders.settings_file(), two).unwrap();
    let settings = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
    let (fetching, session, heard, _headless) = handle_with_headless();
    let db = OpenStore::new("calls-launch-store");
    let parts = CallParts {
        folders: Ok(folders.clone()),
        settings,
        definitions: no_servers(),
    };

    let calls = Calls::launched(fetching, db.reader.clone(), parts);
    let at_launch = (synced_and_saved(&session), heard.clones());
    let filter = "\n[[repositories.filters]]\nname = \"Bugs\"\nquery = \"label:bug\"\n";
    fs::write(folders.settings_file(), format!("{two}{filter}")).unwrap();
    let reread = calls.reread_settings().map(|loaded| loaded.problems.len());
    let after_edit = synced_and_saved(&session);
    db.close();

    let (synced, clones) = at_launch;
    assert_eq!(synced, ["sync a/b"]);
    let names: Vec<String> = clones.iter().map(ToString::to_string).collect();
    assert_eq!(
        names,
        ["a/b", "a/b", "c/d"],
        "the open one's clone goes next"
    );
    assert_eq!(reread.unwrap(), 0, "no problems in the files");
    assert_eq!(after_edit, ["c/d saved by HandEdit, 1 Filter"]);
}

/// Settings that list `repositories` and nothing else.
fn listing(repositories: Vec<RepositorySettings>) -> SettingsView {
    let settings = Settings {
        repositories,
        ..Settings::default()
    };
    SettingsView {
        loaded: LoadedSettings {
            settings,
            ..LoadedSettings::default()
        },
        ..SettingsView::default()
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
    let calls = Setup::new("no-head");
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
    assert_eq!(calls.heard.clones(), [], "the mirror is there");
    assert_eq!(calls.heard.asked(), ["python/cpython awaits #7"]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_head_the_mirror_lacks_fetches_it_first() {
    let calls = Setup::new("missing");
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
    let calls = Setup::new("held");
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
    let calls = Setup::new("moved");
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
    let calls = Setup::new("cloning");
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
    let calls = Setup::new("asking");

    let files = (calls.work()).answer(&calls.world.repository(), 7, FilesRead::Asking);

    assert_eq!(files.unwrap(), PrFiles::Asking);
    assert_eq!(calls.heard.clones(), []);
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_repository_the_settings_dont_list_queues_no_clone() {
    let mut calls = Setup::new("unlisted");
    calls.world.has_pr(None, "main");
    calls.settings.set(&listing(vec![]));

    let problem = calls
        .work()
        .pr_files(&calls.world.repository(), 7)
        .unwrap_err();

    assert_eq!(
        problem.message,
        "Tsugiki keeps no mirror of python/cpython, which the settings don't list."
    );
    assert_eq!(calls.heard.clones(), []);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_pr_whose_conversation_named_no_head_asks_github_no_more() {
    let calls = Setup::new("no-head-named");
    let mirrored = Mirrored::new("calls-no-head-named");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(None, "main");
    calls.world.has_conversation(None);
    let read_this_launch = Item {
        repository: calls.world.repository(),
        number: 7,
    };
    calls.headless.named_none(&read_this_launch);

    let problem = calls
        .work()
        .pr_files(&calls.world.repository(), 7)
        .unwrap_err();

    assert_eq!(
        problem.message,
        "GitHub named no head commit of PR #7 when Tsugiki read its Conversation, so it has no diff to \
         show."
    );
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    assert!(calls.heard.asked().is_empty(), "{:?}", calls.heard.asked());
    calls.finish();
}

#[test]
fn the_files_tab_of_a_pr_whose_conversation_an_older_tsugiki_saved_without_a_head_reads_it_again() {
    let calls = Setup::new("no-head-kept");
    let mirrored = Mirrored::new("calls-no-head-kept");
    calls.world.mirrors(&mirrored);
    calls.world.has_pr(None, "main");
    calls.world.has_conversation(None);

    let files = calls.pr_files();

    assert!(matches!(files, Some(PrFiles::Fetching { .. })), "{files:?}");
    assert_eq!(calls.conversations_read(), [7]);
    calls.finish();
}

#[test]
fn opening_a_pr_reads_its_conversation_though_the_mirrors_folder_cant_be_found() {
    let calls = Setup::new("open-no-folders");
    let missing = Error::new(ErrorKind::NotFound, "No folder for the mirrors.");
    let work = PullRequestWork {
        sources: Err(missing.clone()),
        ..calls.work()
    };

    let opened = work.open_pull_request(&calls.world.repository(), 7);

    assert_eq!(opened.unwrap_err(), missing);
    assert_eq!(calls.conversations_read(), [7]);
    calls.finish();
}

#[test]
fn the_files_tab_of_a_pr_tsugiki_doesnt_hold_shows_nothing_and_asks_nothing() {
    let calls = Setup::new("none");

    assert_eq!(calls.pr_files(), None);
    assert_eq!(calls.heard.clones(), []);
    assert_eq!(calls.conversations_read(), [0_u32; 0]);
    calls.finish();
}

#[test]
fn opening_a_pr_reads_its_conversation_and_fetches_a_head_the_mirror_lacks_first() {
    let calls = Setup::new("open-missing");
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
    let calls = Setup::new("open-no-head");
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
