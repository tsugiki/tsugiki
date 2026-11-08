//! The window's commands and events, set up on any Tauri runtime. `startup.rs` calls this on the real one.
//! The tests here call it on Tauri's mock one, so they check this wiring without a window.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tsugiki_core::diff::{Diffs, Highlighter};
use tsugiki_core::error::Error;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::latest;
use tsugiki_core::mirror::{Announce, Mirrors, Remotes};
use tsugiki_core::session::{Intervals, Session};
use tsugiki_core::settings::{SettingsFiles, SettingsOwner};
use tsugiki_core::status::{Change, Status, StatusBoard};

use crate::{
    Changed, CloningChanged, DiffWork, FolderPicker, HeadsFetched, LastSyncChanged,
    MirrorProblemChanged, MirrorQuestionChanged, PickFolder, RepositoryWork, StatusChanged,
    SyncActivityChanged,
};

/// How often the open Repository refreshes: a refresh costs a few points, and this is about 60 points an
/// hour, out of 5,000 (ADR 0015).
const REPOSITORY_REFRESH: Duration = Duration::from_secs(180);

/// The world outside the window that Tsugiki reaches: GitHub's API and `gh`, where mirrors are cloned from,
/// and the system's folder picker.
pub(crate) struct Outside {
    pub(crate) github: GitHubAccess,
    pub(crate) remotes: Remotes,
    pub(crate) pick_folder: PickFolder,
}

/// The threads that work for the window: the session, and the mirrors' one writer.
pub(crate) struct Core {
    pub(crate) session: Session,
    mirrors: Mirrors,
}

impl Core {
    /// Stops both threads at once, and gives them `patience` between them to finish.
    pub(crate) fn stop(&self, patience: Duration) {
        let deadline = Instant::now() + patience;
        self.mirrors.begin_stopping();
        self.session.stop(patience);
        self.mirrors
            .wait_for_end(deadline.saturating_duration_since(Instant::now()));
    }
}

/// Reads the settings, starts the session and the mirrors, and gives the commands what they ask. Events need
/// `mount_events` to have run on `app`.
pub(crate) fn connect_core<R: Runtime>(
    app: &AppHandle<R>,
    folders: Result<Folders, Error>,
    outside: Outside,
    refresh: Duration,
) -> Core {
    let (mut latest_status, status_reader) = latest::channel(Status::default());
    assert!(app.manage(status_reader), "only this manages the status");
    let (mut latest_sync, last_sync_reader) = latest::channel(None);
    assert!(
        app.manage(last_sync_reader),
        "only this manages the last sync"
    );
    let (mut latest_activity, activity_reader) = latest::channel(None);
    assert!(
        app.manage(activity_reader),
        "only this manages the sync activity"
    );
    assert!(
        app.manage(FolderPicker(outside.pick_folder)),
        "only this manages the folder picker"
    );
    // Two small files, read before the window exists so `settings` and `home` can always answer. A
    // `settings_path` on a slow drive would delay the window. Without folders the defaults apply, and the
    // session shows why. A thread of their own owns the files from here, so saving doesn't wait on GitHub.
    let settings = SettingsOwner::start(folders.clone().map(SettingsFiles::load));
    let status_handle = app.clone();
    let changes_handle = app.clone();
    let last_sync_handle = app.clone();
    let activity_handle = app.clone();
    let announce = mirror_announcements(app);
    let mirrors = Mirrors::start(folders.clone(), outside.remotes, announce);
    let (following, fetching) = (mirrors.requests(), mirrors.requests());
    let board = StatusBoard::new(Box::new(move |status| {
        following.follow(status);
        // Before the event: a window that starts listening too late for it then asks `status()`, and has to
        // get this status.
        latest_status.set(status);
        // A window still loading asks `status()` when it is done, so it doesn't need this event.
        log_if_unsent(StatusChanged(status.clone()).emit(&status_handle));
    }))
    .with_changes(Box::new(move |change| {
        log_if_unsent(Changed(change.clone()).emit(&changes_handle));
    }))
    .with_last_syncs(Box::new(move |last_sync| {
        // Before the event, for the reason the status gives above.
        latest_sync.set(&Some(last_sync.clone()));
        log_if_unsent(LastSyncChanged(last_sync.clone()).emit(&last_sync_handle));
    }))
    .with_sync_activity(Box::new(move |activity| {
        // Before the event, for the reason the status gives above.
        let activity = activity.cloned();
        latest_activity.set(&activity);
        log_if_unsent(SyncActivityChanged(activity).emit(&activity_handle));
    }))
    .with_heads(Box::new(move |repository, heads| {
        fetching.fetch_heads(repository.clone(), heads);
    }));
    let intervals = Intervals {
        status: refresh,
        repository: REPOSITORY_REFRESH,
    };
    let diffs = DiffWork {
        folders: folders.clone(),
        diffs: Diffs::default(),
        mirrors: mirrors.requests(),
        highlighter: Highlighter::start(),
    };
    assert!(app.manage(diffs), "only this manages the diffs");
    let session = Session::start(board, folders, outside.github, intervals);
    let requests = session.requests();
    let work = RepositoryWork {
        session: requests.clone(),
        mirrors: mirrors.requests(),
    };
    let view = settings.view();
    work.start(&view.home, &view.mirror_sources);
    // The Repositories added before 0.2 have no mirror yet, and get theirs after the open one's. One the user
    // hasn't said where its objects come from asks when its turn comes.
    for repository in &view.loaded.settings.repositories {
        let source = view.mirror_sources.get(&repository.name).cloned();
        work.mirrors.clone_mirror(repository.name.clone(), source);
    }
    assert!(app.manage(settings), "only this manages the settings");
    assert!(app.manage(requests), "only this manages the requests");
    assert!(
        app.manage(work),
        "only this manages the work on Repositories"
    );
    let reader = session.reader();
    assert!(app.manage(reader), "only this manages the reader");
    Core { session, mirrors }
}

/// Tells the window what the mirrors' one writer says. Each change is set for the commands that ask before it
/// goes out as an event, for the reason `connect_core` gives for the status.
fn mirror_announcements<R: Runtime>(app: &AppHandle<R>) -> Announce {
    let (mut latest_cloning, cloning_reader) = latest::channel(None);
    assert!(app.manage(cloning_reader), "only this manages the clone");
    let (mut latest_problem, problem_reader) = latest::channel(None);
    assert!(
        app.manage(problem_reader),
        "only this manages the mirror's problem"
    );
    let (mut latest_question, question_reader) = latest::channel(None);
    assert!(
        app.manage(question_reader),
        "only this manages the mirror's question"
    );
    let cloning_handle = app.clone();
    let problem_handle = app.clone();
    let question_handle = app.clone();
    let heads_handle = app.clone();
    Announce {
        cloning: Box::new(move |cloning| {
            let cloning = cloning.cloned();
            latest_cloning.set(&cloning);
            log_if_unsent(CloningChanged(cloning).emit(&cloning_handle));
        }),
        fetched: Box::new(move |repository, numbers| {
            let change = Change {
                repository: repository.clone(),
                items: numbers.to_vec(),
            };
            log_if_unsent(HeadsFetched(change).emit(&heads_handle));
        }),
        problem: Box::new(move |problem| {
            let problem = problem.cloned();
            latest_problem.set(&problem);
            log_if_unsent(MirrorProblemChanged(problem).emit(&problem_handle));
        }),
        question: Box::new(move |repository| {
            let repository = repository.cloned();
            latest_question.set(&repository);
            log_if_unsent(MirrorQuestionChanged(repository).emit(&question_handle));
        }),
    }
}

/// A window that misses an event catches up when it next asks, so this only leaves a trace in the log.
fn log_if_unsent(sent: tauri::Result<()>) {
    if let Err(cause) = sent {
        log::warn!("Tsugiki can't tell the window about a change: {cause}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::specta_builder;
    use crate::tests::{cpython, no_folders, temp_folders};
    use serde_json::{Value, json};
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{App, Listener, WebviewWindow};
    use tsugiki_core::github::pull_requests::{PrState, PullRequestFacts};
    use tsugiki_core::github::replay::Replayer;
    use tsugiki_core::github::{CommitSha, Login};
    use tsugiki_core::github::{Reply, Request, Transport, Unreachable};
    use tsugiki_core::mirror::MirrorSource;
    use tsugiki_core::repository::RepositoryAccess;
    use tsugiki_core::settings::SettingsFiles;
    use tsugiki_core::token::{GhOutput, Token};
    use tsugiki_core::web::{GitHubItem, WebAddress};

    /// Longer than any of these tests should wait on a working app.
    const PLENTY: Duration = Duration::from_secs(60);

    fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
        Ok(GhOutput {
            success: true,
            stdout: "gho_example".to_owned(),
        })
    }

    /// Real GitHub's answers, recorded for the core's fixtures.
    fn recordings() -> GitHubAccess {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/github");
        GitHubAccess {
            transport: Box::new(Replayer::new(dir)),
            run_gh: signed_in_gh,
        }
    }

    struct NoNetwork;

    impl Transport for NoNetwork {
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            Err(Unreachable::Offline)
        }
    }

    fn no_network() -> GitHubAccess {
        GitHubAccess {
            transport: Box::new(NoNetwork),
            run_gh: signed_in_gh,
        }
    }

    /// Says when a request arrives, then holds it until the test lets go. A regression then shows up as
    /// a slow test that still ends.
    struct StuckGitHub {
        arrived: Sender<()>,
        released: Receiver<()>,
    }

    impl Transport for StuckGitHub {
        fn post(&self, _: &Token, _: &Request<'_>) -> Result<Reply, Unreachable> {
            self.arrived.send(()).unwrap_or(());
            self.released.recv_timeout(PLENTY).unwrap_or(());
            Err(Unreachable::Offline)
        }
    }

    /// The app as `startup.rs` wires it, on Tauri's mock runtime with a webview to send commands from. It
    /// listens before the session starts, so it hears every event.
    struct Wired {
        core: Core,
        webview: WebviewWindow<MockRuntime>,
        statuses: Receiver<Value>,
        workbenches: Receiver<Value>,
        last_syncs: Receiver<Value>,
        activities: Receiver<Value>,
        clonings: Receiver<Value>,
        problems: Receiver<Value>,
        questions: Receiver<Value>,
        heads: Receiver<Value>,
        _app: App<MockRuntime>,
    }

    impl Wired {
        /// The app with mirrors cloned from a folder with nothing in it, so each clone fails at once.
        fn start(folders: Result<Folders, Error>, access: GitHubAccess) -> Self {
            Self::start_cloning_from(folders, access, Remotes::under("no-remotes/".to_owned()))
        }

        fn start_cloning_from(
            folders: Result<Folders, Error>,
            github: GitHubAccess,
            remotes: Remotes,
        ) -> Self {
            Self::start_picking(folders, github, remotes, Arc::new(|| None))
        }

        /// The app whose folder picker gives what `pick_folder` gives.
        fn start_picking(
            folders: Result<Folders, Error>,
            github: GitHubAccess,
            remotes: Remotes,
            pick_folder: PickFolder,
        ) -> Self {
            let builder = specta_builder();
            let app = mock_builder()
                .invoke_handler(builder.invoke_handler())
                .build(mock_context(noop_assets()))
                .unwrap();
            builder.mount_events(&app);
            let statuses = hear(&app, "status-changed");
            let workbenches = hear(&app, "changed");
            let last_syncs = hear(&app, "last-sync-changed");
            let activities = hear(&app, "sync-activity-changed");
            let clonings = hear(&app, "cloning-changed");
            let problems = hear(&app, "mirror-problem-changed");
            let questions = hear(&app, "mirror-question-changed");
            let heads = hear(&app, "heads-fetched");
            let outside = Outside {
                github,
                remotes,
                pick_folder,
            };
            let core = connect_core(app.handle(), folders, outside, Duration::from_secs(3600));
            let webview =
                tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
                    .build()
                    .unwrap();
            Self {
                core,
                webview,
                statuses,
                workbenches,
                last_syncs,
                activities,
                clonings,
                problems,
                questions,
                heads,
                _app: app,
            }
        }

        /// The webview's answer to `command`, through Tauri's own handling of its arguments and threads.
        #[expect(
            clippy::unwrap_in_result,
            reason = "the `Result` is the command's answer, and anything else should fail the test"
        )]
        fn ask(&self, command: &str, arguments: Value) -> Result<Value, Value> {
            tauri::test::get_ipc_response(
                &self.webview,
                tauri::webview::InvokeRequest {
                    cmd: command.to_owned(),
                    callback: tauri::ipc::CallbackFn(0),
                    error: tauri::ipc::CallbackFn(1),
                    url: self.webview.url().unwrap(),
                    body: tauri::ipc::InvokeBody::Json(arguments),
                    headers: tauri::http::HeaderMap::default(),
                    invoke_key: tauri::test::INVOKE_KEY.to_owned(),
                },
            )
            .map(|answer| answer.deserialize().unwrap())
        }

        fn stop(self) -> Self {
            self.core.stop(PLENTY);
            self
        }
    }

    /// Each payload of `event`, as the webview would get it.
    fn hear(app: &App<MockRuntime>, event: &str) -> Receiver<Value> {
        let (heard, payloads) = channel();
        let _ = app.listen(event, move |event| {
            drop(heard.send(serde_json::from_str(event.payload()).unwrap()));
        });
        payloads
    }

    const CPYTHON: &str = "python/cpython";

    fn add_cpython() -> Value {
        json!({ "repository": { "name": CPYTHON, "access": "other", "source": { "kind": "download" } } })
    }

    fn the_whole_workbench() -> Value {
        json!({ "repository": CPYTHON, "view": { "kind": "whole" } })
    }

    #[test]
    fn with_no_folders_every_command_that_needs_them_says_so_in_the_cores_form() {
        let wired = Wired::start(Err(no_folders()), no_network());

        let checking = wired.ask("check_repository", json!({ "input": "a/b" }));
        let adding = wired.ask("add_repository", add_cpython());
        let reading = wired.ask(
            "workbench",
            json!({ "repository": "a/b", "view": { "kind": "whole" } }),
        );
        let files = wired.ask(
            "pull_request_files",
            json!({ "repository": "a/b", "number": 1 }),
        );
        drop(wired.stop());

        let checking = checking.unwrap_err();
        assert_eq!(checking["kind"], "offline");
        assert!(
            checking["message"]
                .as_str()
                .unwrap()
                .contains("isn't connected")
        );
        let adding = adding.unwrap_err();
        assert_eq!(adding["kind"], "internal");
        assert_eq!(adding["message"], no_folders().message);
        assert_eq!(reading.unwrap_err()["kind"], "internal");
        assert_eq!(files.unwrap_err()["message"], no_folders().message);
    }

    #[test]
    fn the_add_dialogs_check_goes_to_the_session_and_back() {
        let (folders, root) = temp_folders("wired-check");
        let wired = Wired::start(Ok(folders), no_network());

        let answer = wired.ask("check_repository", json!({ "input": CPYTHON }));
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        let answer = answer.unwrap_err();
        assert_eq!(answer["kind"], "offline");
        assert_eq!(answer["message"], "Tsugiki can't reach GitHub.");
    }

    #[test]
    fn an_add_and_a_read_dont_wait_for_github() {
        let (folders, root) = temp_folders("wired-stuck");
        let (arrived, request_arrived) = channel();
        let (release, released) = channel();
        let access = GitHubAccess {
            transport: Box::new(StuckGitHub { arrived, released }),
            run_gh: signed_in_gh,
        };
        let wired = Wired::start(Ok(folders), access);
        request_arrived.recv_timeout(PLENTY).unwrap();

        let started = Instant::now();
        let added = wired.ask("add_repository", add_cpython());
        let read = wired.ask("workbench", the_whole_workbench());
        let waited = started.elapsed();
        drop(release);
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap()["screen"], "repository");
        assert_eq!(read.unwrap()["items"], json!([]));
        assert!(waited < PLENTY / 2, "{waited:?}");
    }

    #[test]
    fn the_panes_widths_stay_on_this_machine_across_a_restart() {
        let (folders, root) = temp_folders("wired-panes");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let before = wired.ask("pane_widths", json!({})).unwrap();
        let widths = json!({ "sidebar": 240, "list": null });
        let saved = wired.ask("save_pane_widths", json!({ "widths": widths }));
        drop(wired.stop());

        let restarted = Wired::start(Ok(folders.clone()), no_network());
        let reopened = restarted.ask("pane_widths", json!({})).unwrap();
        drop(restarted.stop());
        let synced = folders.settings_file().exists();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(before, json!({ "sidebar": null, "list": null }));
        assert_eq!(
            (saved.unwrap(), reopened, synced),
            (widths.clone(), widths, false)
        );
    }

    #[test]
    fn with_no_folders_the_panes_widths_arent_saved() {
        let wired = Wired::start(Err(no_folders()), no_network());

        let saved = wired.ask("save_pane_widths", json!({ "widths": { "sidebar": 240 } }));
        drop(wired.stop());

        assert_eq!(saved.unwrap_err()["kind"], "internal");
    }

    #[test]
    fn a_repository_added_reaches_both_files_and_opens_after_a_restart() {
        let (folders, root) = temp_folders("wired-restart");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let before = wired.ask("home", json!({})).unwrap();
        let added = wired.ask("add_repository", add_cpython()).unwrap();
        let after = wired.ask("home", json!({})).unwrap();
        drop(wired.stop());

        let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
        let machine = std::fs::read_to_string(folders.machine_file()).unwrap();
        let restarted = Wired::start(Ok(folders), no_network());
        let reopened = restarted.ask("home", json!({})).unwrap();
        drop(restarted.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(before["screen"], "onboarding");
        assert_eq!(added, json!(cpython()));
        assert_eq!(after, added);
        assert_eq!(reopened, added);
        assert!(
            settings.contains(r#"name = "python/cpython""#),
            "{settings}"
        );
        assert!(
            settings.contains(r#"conventions = "cpython""#),
            "{settings}"
        );
        assert!(
            machine.contains(r#"last_repository = "python/cpython""#),
            "{machine}"
        );
    }

    #[test]
    fn a_window_that_loads_after_an_announcement_gets_the_same_status() {
        let (folders, root) = temp_folders("wired-status");
        let wired = Wired::start(Ok(folders), no_network());

        let announced = wired.statuses.recv_timeout(PLENTY).unwrap();
        let asked = wired.ask("status", json!({})).unwrap();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(announced, json!({ "state": "offline" }));
        assert_eq!(asked, announced);
    }

    /// A system browser that won't start.
    fn no_browser(_: &WebAddress) -> Result<(), Error> {
        Err(Error::new(
            tsugiki_core::error::ErrorKind::Internal,
            "no browser",
        ))
    }

    #[test]
    fn opening_an_issue_moves_its_last_seen_and_tells_the_window() {
        let (folders, root) = temp_folders("wired-open-issue");
        let wired = Wired::start(Ok(folders), no_network());
        let issue_7 = json!({ "repository": CPYTHON, "number": 7 });

        drop(wired.ask("open_issue", issue_7.clone()).unwrap());
        let changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
        drop(wired.ask("fetch_issue", issue_7.clone()).unwrap());
        let held = wired.ask("issue", issue_7).unwrap();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(changed, json!({ "repository": CPYTHON, "items": [7] }));
        assert_eq!(held, Value::Null, "nothing has saved an Issue #7");
    }

    #[test]
    fn opening_a_pr_shows_its_row_at_once_and_its_conversation_once_github_answers() {
        let (folders, root) = temp_folders("wired-open-pr");
        let wired = Wired::start(Ok(folders), recordings());
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let workbench = workbench_after_sync(&wired);
        let number = workbench["items"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|item| item["pull_requests"][0]["number"].as_u64())
            .unwrap();
        let pr = json!({ "repository": CPYTHON, "number": number });

        let before = wired.ask("pull_request", pr.clone()).unwrap();
        drop(wired.ask("open_pull_request", pr.clone()).unwrap());
        let mut after = Value::Null;
        while after["content"].is_null() {
            let _changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
            after = wired.ask("pull_request", pr.clone()).unwrap();
        }
        drop(wired.ask("fetch_pull_request", pr).unwrap());
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(before["number"], number);
        assert_eq!(before["content"], Value::Null);
        let description = after["content"]["body_html"].as_str().unwrap();
        assert!(description.contains("gh-152315"), "{description}");
        let first = &after["content"]["reviews"][0];
        assert_eq!(
            (&first["state"], first["threads"].as_array().map(Vec::len)),
            (&json!("COMMENTED"), Some(5)),
            "the recorded PR's oldest Review, with the threads it started"
        );
    }

    #[test]
    fn a_name_that_isnt_a_repository_opens_nothing() {
        let (folders, root) = temp_folders("wired-open-bad-name");
        let wired = Wired::start(Ok(folders), no_network());
        let bad = json!({ "repository": "not a name", "number": 7 });

        let open = wired.ask("open_issue", bad.clone()).unwrap_err();
        let fetch = wired.ask("fetch_issue", bad.clone()).unwrap_err();
        let read = wired.ask("issue", bad.clone()).unwrap_err();
        let pr_errors: Vec<Value> = ["open_pull_request", "fetch_pull_request", "pull_request"]
            .into_iter()
            .map(|command| wired.ask(command, bad.clone()).unwrap_err()["kind"].clone())
            .collect();
        let requests = wired.core.session.requests();
        let page = crate::open_github_page(
            "not a name",
            GitHubItem::Issue { number: 7 },
            &requests,
            no_browser,
        );
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(open["kind"], "not_found");
        assert_eq!(fetch["kind"], "not_found");
        assert_eq!(read["kind"], "not_found");
        assert_eq!(pr_errors, ["not_found", "not_found", "not_found"]);
        assert!(page.is_err());
    }

    #[test]
    fn opening_an_item_on_github_shows_its_page_and_then_moves_its_last_seen() {
        let (folders, root) = temp_folders("wired-open-on-github");
        let wired = Wired::start(Ok(folders), no_network());
        let requests = wired.core.session.requests();
        let shown = std::cell::RefCell::new(Vec::new());
        for item in [
            GitHubItem::Issue { number: 7 },
            GitHubItem::PullRequest { number: 8 },
        ] {
            crate::open_github_page(CPYTHON, item, &requests, |address| {
                shown.borrow_mut().push(address.as_str().to_owned());
                Ok(())
            })
            .unwrap();
        }
        let first = wired.workbenches.recv_timeout(PLENTY).unwrap();
        let second = wired.workbenches.recv_timeout(PLENTY).unwrap();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(
            shown.into_inner(),
            [
                "https://github.com/python/cpython/issues/7",
                "https://github.com/python/cpython/pull/8",
            ]
        );
        assert_eq!(first, json!({ "repository": CPYTHON, "items": [7] }));
        assert_eq!(second, json!({ "repository": CPYTHON, "items": [8] }));
    }

    #[test]
    fn a_browser_that_doesnt_open_leaves_last_seen_where_it_is() {
        let (folders, root) = temp_folders("wired-no-browser");
        let wired = Wired::start(Ok(folders), no_network());
        let requests = wired.core.session.requests();

        let opened = crate::open_github_page(
            CPYTHON,
            GitHubItem::PullRequest { number: 8 },
            &requests,
            no_browser,
        );
        let wired = wired.stop();
        std::fs::remove_dir_all(root).unwrap();

        assert!(opened.is_err());
        assert!(wired.workbenches.try_recv().is_err());
    }

    /// Each item the answer lists, in the order the window shows them.
    fn listed(workbench: &Value) -> Vec<&Value> {
        let items = workbench["items"].as_array().unwrap().iter();
        items
            .chain(workbench["quiet"].as_array().unwrap())
            .collect()
    }

    /// The recordings hold 21 PRs of real CPython work, and 3 of them are the recording user's own. They name
    /// 21 Issues: #154019 fixes two by itself, and #156423 and #156419 fix one between them. The Filters'
    /// search reaches 2 more that no PR names.
    fn assert_is_the_recorded_work(workbench: &Value) {
        let items = listed(workbench);
        assert_eq!(items.len(), 23);
        assert!(
            items.iter().all(|item| item["kind"] == "issue"),
            "{items:?}"
        );
        let entries: Vec<&Value> = items
            .iter()
            .flat_map(|item| item["pull_requests"].as_array().unwrap())
            .collect();
        let rows: Vec<&Value> = entries
            .iter()
            .flat_map(|entry| {
                let beneath = entry["backports"].as_array().into_iter().flatten();
                std::iter::once(*entry).chain(beneath)
            })
            .collect();
        let under_154019 = rows.iter().filter(|row| row["number"] == 154_019);
        assert_eq!(under_154019.count(), 2);
        assert_the_recorded_tree_has_its_backports(&entries);
        let with_reason = |reason: &str| {
            rows.iter()
                .filter(|row| row["reasons"].as_array().unwrap().contains(&json!(reason)))
                .count()
        };
        assert_eq!(with_reason("own_pr"), 3);
        assert!(with_reason("review_request") > 0);
    }

    /// One Issue's tree comes back from real GitHub: a merged Origin PR with the bot's Backports and one a
    /// person opened beneath it.
    fn assert_the_recorded_tree_has_its_backports(entries: &[&Value]) {
        let origin = entries
            .iter()
            .find(|entry| entry["number"] == 151_968)
            .unwrap();
        assert_eq!(origin["kind"], "pr");
        assert_eq!(origin["state"], "MERGED");
        let kind_of = |number: u32| {
            let backports = origin["backports"].as_array().unwrap();
            let found = backports.iter().find(|row| row["number"] == number);
            found.unwrap()["backport"].clone()
        };
        assert_eq!(kind_of(152_837), "bot");
        assert_eq!(kind_of(152_840), "manual");
    }

    /// Waits for the session to save what each starter Filter found, the PRs, their Issues, one Issue's tree
    /// and the Origin PR that tree was missing, and returns the Workbench after.
    fn workbench_after_sync(wired: &Wired) -> Value {
        for _ in ["Filter", "Filter", "PRs", "Issues", "tree", "Origin PR"] {
            let changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
            assert_eq!(changed["repository"], CPYTHON);
        }
        wired.ask("workbench", the_whole_workbench()).unwrap()
    }

    #[test]
    fn adding_a_repository_brings_in_what_waits_on_the_user() {
        let (folders, root) = temp_folders("wired-first-sync");
        let wired = Wired::start(Ok(folders), recordings());

        let added = wired.ask("add_repository", add_cpython());
        let workbench = workbench_after_sync(&wired);
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), json!(cpython()));
        assert_is_the_recorded_work(&workbench);
    }

    #[test]
    fn the_app_syncs_the_repository_it_reopens() {
        let (folders, root) = temp_folders("wired-reopen");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        drop(wired.stop());

        let reopened = Wired::start(Ok(folders), recordings());
        let workbench = workbench_after_sync(&reopened);
        drop(reopened.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_is_the_recorded_work(&workbench);
    }

    #[test]
    fn a_filter_edited_in_the_file_shows_in_the_sidebar_and_the_workbench_after_a_restart() {
        const EDITED: &str = "is:open author:@me label:type-bug";
        let (folders, root) = temp_folders("wired-filter-edit");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        drop(wired.stop());
        let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
        let edited = settings
            .replace(r#"name = "Mine""#, r#"name = "My bugs""#)
            .replace(
                r#"query = "is:open author:@me""#,
                &format!(r#"query = "{EDITED}""#),
            );
        std::fs::write(folders.settings_file(), edited).unwrap();

        let reopened = Wired::start(Ok(folders), recordings());
        let whole = workbench_after_sync(&reopened);
        let selected = reopened
            .ask(
                "workbench",
                json!({ "repository": CPYTHON, "view": { "kind": "filter", "query": EDITED } }),
            )
            .unwrap();
        drop(reopened.stop());
        std::fs::remove_dir_all(root).unwrap();

        let line = &whole["filters"][0];
        assert_eq!(line["name"], "My bugs");
        assert_eq!(line["query"], EDITED);
        assert_eq!(selected["filters"], whole["filters"]);
        // The search found 7 items. #157178 shares a row with the Issue it fixes, which the search found too,
        // and #156423 and #156419 share one with the Issue they fix.
        assert_eq!(listed(&selected).len(), 5);
    }

    #[test]
    fn a_maintained_repository_starts_with_its_own_filters() {
        let (folders, root) = temp_folders("wired-maintained");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let adding = json!({
            "repository": { "name": CPYTHON, "access": "maintained", "source": { "kind": "download" } },
        });

        let added = wired.ask("add_repository", adding).unwrap();
        drop(wired.stop());
        let settings = std::fs::read_to_string(folders.settings_file()).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added["repository"]["filters"][0]["name"], "Open Issues");
        assert!(settings.contains(r#"name = "Open PRs""#), "{settings}");
    }

    /// Answers a refresh in which Issue `number` got a new title, and says what it was asked.
    struct Refreshing {
        number: u64,
        asked: Sender<String>,
    }

    impl Transport for Refreshing {
        fn post(&self, _: &Token, request: &Request<'_>) -> Result<Reply, Unreachable> {
            drop(self.asked.send(request.operation.to_owned()));
            let rate = json!({ "remaining": 4000, "resetAt": "2999-01-01T00:00:00Z" });
            let page = |nodes: Value| json!({ "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": nodes });
            let data = match request.operation {
                "Viewer" => json!({ "viewer": { "login": "octocat" } }),
                "RefreshRepository" => json!({
                    "repository": {
                        "openIssues": { "totalCount": 9000 },
                        "openPrs": { "totalCount": 0 },
                        "issues": page(json!([{
                            "number": self.number,
                            "title": "A new title",
                            "updatedAt": "2999-01-01T00:00:00Z",
                            "state": "OPEN",
                            "stateReason": null,
                            "assignees": { "nodes": [] },
                            "timelineItems": { "nodes": [] },
                            "author": null,
                        }])),
                        "pullRequests": page(json!([])),
                    },
                    "rateLimit": rate,
                }),
                "FetchCrossReferences" => {
                    json!({ "repository": { "issue": null }, "rateLimit": rate })
                }
                _ => json!({ "repository": {}, "rateLimit": rate }),
            };
            Ok(Reply {
                status: 200,
                reset: None,
                date: None,
                body: json!({ "data": data }).to_string(),
            })
        }
    }

    fn wait_for(asked: &Receiver<String>, operation: &str) {
        while asked.recv_timeout(PLENTY).unwrap() != operation {}
    }

    /// The first thing the first item of a Workbench says about itself.
    fn first_issue(workbench: &Value) -> &Value {
        &workbench["items"][0]
    }

    #[test]
    fn a_later_start_refreshes_shows_the_change_and_a_click_asks_again() {
        let (folders, root) = temp_folders("wired-refresh");
        let wired = Wired::start(Ok(folders.clone()), recordings());
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let workbench = workbench_after_sync(&wired);
        let never = wired.last_syncs.recv_timeout(PLENTY).unwrap();
        let first = wired.last_syncs.recv_timeout(PLENTY).unwrap();
        drop(wired.stop());
        let number = first_issue(&workbench)["number"].as_u64().unwrap();

        let (asked, questions) = channel();
        let access = GitHubAccess {
            transport: Box::new(Refreshing { number, asked }),
            run_gh: signed_in_gh,
        };
        let reopened = Wired::start(Ok(folders), access);
        let shown = reopened.last_syncs.recv_timeout(PLENTY).unwrap();
        let changed = reopened.workbenches.recv_timeout(PLENTY).unwrap();
        let after = reopened.ask("workbench", the_whole_workbench()).unwrap();
        let refreshed = reopened.last_syncs.recv_timeout(PLENTY).unwrap();
        let asked_for_it = reopened.ask("last_sync", json!({})).unwrap();
        wait_for(&questions, "RefreshRepository");
        let clicked = reopened.ask("refresh", json!({}));
        wait_for(&questions, "RefreshRepository");
        drop(reopened.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(never, json!({ "repository": CPYTHON, "at": null }));
        assert_eq!(first["repository"], CPYTHON);
        assert_eq!(shown, first, "a later start says when the last sync was");
        assert_eq!(changed, json!({ "repository": CPYTHON, "items": [number] }));
        assert_eq!(first_issue(&after)["title"], "A new title");
        assert!(refreshed["at"].is_string(), "{refreshed}");
        assert_eq!(asked_for_it, refreshed);
        assert_eq!(clicked.unwrap(), Value::Null);
    }

    #[test]
    fn the_status_bar_hears_each_part_of_a_first_sync_and_then_that_it_ended() {
        let (folders, root) = temp_folders("wired-activity");
        let wired = Wired::start(Ok(folders), recordings());
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let _workbench = workbench_after_sync(&wired);

        let mut parts = Vec::new();
        let ended = loop {
            let heard = wired.activities.recv_timeout(PLENTY).unwrap();
            if heard.is_null() {
                break heard;
            }
            parts.push(heard["open"]["part"].as_str().unwrap().to_owned());
        };
        parts.dedup();
        let asked = wired.ask("sync_activity", json!({})).unwrap();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(parts, ["refresh", "your_work", "trees", "window"]);
        assert_eq!(ended, Value::Null);
        assert_eq!(asked, Value::Null);
    }

    #[test]
    fn a_window_that_loads_before_the_core_knows_hears_no_last_sync() {
        let wired = Wired::start(Err(no_folders()), no_network());

        let asked = wired.ask("last_sync", json!({}));
        let clicked = wired.ask("refresh", json!({}));
        drop(wired.stop());

        assert_eq!(asked.unwrap(), Value::Null);
        assert_eq!(clicked.unwrap(), Value::Null);
    }

    #[test]
    fn with_no_repository_the_app_has_nothing_to_sync() {
        let (folders, root) = temp_folders("wired-nothing");
        let wired = Wired::start(Ok(folders), recordings());

        drop(wired.statuses.recv_timeout(PLENTY).unwrap());
        let wired = wired.stop();
        std::fs::remove_dir_all(root).unwrap();

        assert!(wired.workbenches.try_recv().is_err());
    }

    #[test]
    fn a_change_the_window_cant_hear_leaves_only_a_trace() {
        log_if_unsent(Err(tauri::Error::WindowNotFound));
    }

    #[expect(
        clippy::disallowed_types,
        reason = "tests build their repositories with the real git (ADR 0009)"
    )]
    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }

    /// Bare repositories with one commit each under `root`, laid out as GitHub's are, for mirrors to clone.
    fn remotes(root: &Path, names: &[&str]) -> Remotes {
        let remotes = root.join("remotes");
        for name in names {
            let work = remotes.join("work").join(name);
            std::fs::create_dir_all(&work).unwrap();
            git(&work, &["init", "-q", "-b", "main"]);
            std::fs::write(
                work.join("README.rst"),
                "This is Python version 3.15.0 alpha 0\n",
            )
            .unwrap();
            git(&work, &["add", "README.rst"]);
            git(
                &work,
                &[
                    "-c",
                    "user.name=Contributor",
                    "-c",
                    "user.email=contributor@example.com",
                    "commit",
                    "-qm",
                    "One",
                ],
            );
            let bare = remotes.join(format!("{name}.git"));
            git(
                &remotes,
                &[
                    "clone",
                    "-q",
                    "--bare",
                    &work.to_string_lossy(),
                    &bare.to_string_lossy(),
                ],
            );
        }
        let path = remotes.to_string_lossy().replace('\\', "/");
        let slash = if path.starts_with('/') { "" } else { "/" };
        Remotes::under(format!("file://{slash}{path}/"))
    }

    /// The Repositories each clone announced, in turn, as the webview heard them.
    fn clones(wired: &Wired, count: usize) -> Vec<Vec<Value>> {
        (0..count)
            .map(|_| {
                let mut clone = Vec::new();
                loop {
                    let heard = wired.clonings.recv_timeout(PLENTY).unwrap();
                    if heard.is_null() {
                        break clone;
                    }
                    clone.push(heard);
                }
            })
            .collect()
    }

    #[test]
    fn adding_a_repository_shows_its_clone_until_it_ends() {
        let (folders, root) = temp_folders("wiring-clone");
        let remotes = remotes(&root, &[CPYTHON]);
        let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);

        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let clone = clones(&wired, 1).remove(0);
        let asked = wired.ask("cloning", json!({})).unwrap();
        drop(wired.stop());
        let mirror =
            folders.mirror(&tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap());
        let cloned = mirror.join("HEAD").is_file();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(
            clone.first(),
            Some(&json!({ "repository": CPYTHON, "progress": { "stage": "starting" } }))
        );
        assert!(clone.contains(
            &json!({ "repository": CPYTHON, "progress": { "stage": "receiving", "percent": 100 } })
        ));
        assert_eq!(asked, Value::Null);
        assert!(cloned);
    }

    #[test]
    fn a_disk_short_of_room_for_a_clone_says_so_and_check_again_looks_once_more() {
        let (folders, root) = temp_folders("wiring-disk-short");
        let (measuring, measures) = channel();
        let remotes = Remotes::under("no-remotes/".to_owned()).on_disk(
            Box::new(|_| Ok(5 << 30)),
            Box::new(move |_| {
                measuring.send(()).unwrap_or_default();
                Ok(1 << 30)
            }),
        );
        let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let short = wired.problems.recv_timeout(PLENTY).unwrap();
        let asked = wired.ask("mirror_problem", json!({})).unwrap();
        let _ = wired.ask("check_mirrors_again", json!({})).unwrap();
        let measured = measures
            .recv_timeout(PLENTY)
            .and(measures.recv_timeout(PLENTY));
        // The check before Check again's measure would have said so by now.
        let heard = wired.problems.try_recv();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(short["kind"], "disk_short");
        assert_eq!(short["repository"], CPYTHON);
        assert_eq!(short["needed_mb"], 6 * 1024);
        assert_eq!(short["free_mb"], 1024);
        assert_eq!(asked, short);
        assert!(measured.is_ok(), "Check again measures the disk once more");
        assert!(
            heard.is_err(),
            "a disk still short is nothing new: {heard:?}"
        );
    }

    #[test]
    fn a_repository_from_before_asks_where_its_mirror_gets_objects_and_borrows_on_the_answer() {
        let (folders, root) = temp_folders("wiring-borrow");
        let mut files = SettingsFiles::load(folders.clone());
        let cpython = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
        let _ = files
            .add_repository(&cpython, RepositoryAccess::Other, &MirrorSource::Download)
            .unwrap();
        // As 0.1 left it: the Repository, and no answer.
        std::fs::write(
            folders.machine_file(),
            "last_repository = \"python/cpython\"\n",
        )
        .unwrap();
        let remotes = remotes(&root, &[CPYTHON]);
        let lender = root.join("user-cpython");
        let url = remotes.url(&cpython);
        git(&root, &["clone", "-q", &url, &lender.to_string_lossy()]);
        let picked = lender.clone();
        let wired = Wired::start_picking(
            Ok(folders.clone()),
            recordings(),
            remotes,
            Arc::new(move || Some(picked.clone())),
        );

        let question = wired.questions.recv_timeout(PLENTY).unwrap();
        let asked = wired.ask("mirror_question", json!({})).unwrap();
        let picked = wired.ask("pick_folder", json!({})).unwrap();
        let folder = |path: &Path| json!({ "repository": CPYTHON, "folder": path });
        let refused = wired
            .ask("check_clone", folder(&root.join("gone")))
            .unwrap();
        let lends = wired.ask("check_clone", folder(&lender)).unwrap();
        let borrow = json!({ "kind": "borrow", "folder": lender });
        let chose = wired.ask(
            "choose_mirror_source",
            json!({ "repository": CPYTHON, "source": borrow }),
        );
        let answered = wired.questions.recv_timeout(PLENTY).unwrap();
        let cloned = clones(&wired, 1);
        drop(wired.stop());
        let machine = std::fs::read_to_string(folders.machine_file()).unwrap();
        let alternates = folders
            .mirror(&cpython)
            .join("objects/info/alternates")
            .exists();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(question, json!(CPYTHON));
        assert_eq!(asked, json!(CPYTHON));
        assert_eq!(picked, json!(lender));
        assert_eq!(refused, json!({ "kind": "not_a_folder" }));
        assert_eq!(lends, Value::Null);
        assert_eq!(chose, Ok(Value::Null));
        assert_eq!(answered, Value::Null);
        assert_eq!(cloned.len(), 1);
        assert!(machine.contains("[mirrors]"), "{machine}");
        assert!(!alternates);
    }

    #[test]
    fn an_answer_machine_toml_cant_keep_still_starts_the_clone() {
        let (folders, root) = temp_folders("wiring-answer-unsaved");
        let mut files = SettingsFiles::load(folders.clone());
        let cpython = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
        let _ = files
            .add_repository(&cpython, RepositoryAccess::Other, &MirrorSource::Download)
            .unwrap();
        std::fs::write(
            folders.machine_file(),
            "last_repository = \"python/cpython\"\n",
        )
        .unwrap();
        let wired = Wired::start_cloning_from(
            Ok(folders.clone()),
            recordings(),
            remotes(&root, &[CPYTHON]),
        );

        let _ = wired.questions.recv_timeout(PLENTY).unwrap();
        // Broken after the launch read it, as a user's edit in another window would leave it.
        std::fs::write(folders.machine_file(), "last_repository = \n").unwrap();
        let chose = wired.ask(
            "choose_mirror_source",
            json!({ "repository": CPYTHON, "source": { "kind": "download" } }),
        );
        let answered = wired.questions.recv_timeout(PLENTY).unwrap();
        let cloned = clones(&wired, 1);
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert!(chose.is_err(), "{chose:?}");
        assert_eq!(answered, Value::Null);
        assert_eq!(cloned.len(), 1);
    }

    #[test]
    fn a_picker_the_user_cancels_gives_nothing() {
        let wired = Wired::start(Err(no_folders()), no_network());

        let picked = wired.ask("pick_folder", json!({}));
        drop(wired.stop());

        assert_eq!(picked, Ok(Value::Null));
    }

    #[test]
    fn repositories_added_before_get_their_mirrors_at_launch_the_open_one_first() {
        let (folders, root) = temp_folders("wiring-clones-at-launch");
        let mut files = SettingsFiles::load(folders.clone());
        for name in ["octo-org/first", CPYTHON] {
            let name = tsugiki_core::repository::RepositoryName::parse(name).unwrap();
            let _ = files
                .add_repository(&name, RepositoryAccess::Other, &MirrorSource::Download)
                .unwrap();
        }
        let remotes = remotes(&root, &["octo-org/first", CPYTHON]);
        let wired = Wired::start_cloning_from(Ok(folders), recordings(), remotes);

        let order: Vec<Value> = clones(&wired, 2)
            .into_iter()
            .map(|clone| clone[0]["repository"].clone())
            .collect();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(order, [json!(CPYTHON), json!("octo-org/first")]);
    }

    /// Searches the diff of PR 1 in python/cpython between `head` and `base` for `query`.
    fn find_in_pr_1(
        wired: &Wired,
        (head, base): (&str, &str),
        query: &str,
    ) -> Result<Value, Value> {
        let search = json!({ "repository": CPYTHON, "number": 1, "head": head, "base": base, "query": query });
        wired.ask("pull_request_find", json!({ "search": search }))
    }

    /// Puts a commit of PR `number`'s own on the remote for `name`, under `refs/pull/N/head` as GitHub
    /// keeps it, and gives the commit.
    fn pull_request_on(root: &Path, name: &str, number: u32) -> String {
        let work = root.join("remotes").join("work").join(name);
        std::fs::write(work.join("NEWS"), format!("PR {number}\n")).unwrap();
        git(&work, &["add", "NEWS"]);
        git(
            &work,
            &[
                "-c",
                "user.name=Contributor",
                "-c",
                "user.email=contributor@example.com",
                "commit",
                "-qm",
                "PR",
            ],
        );
        let bare = root.join("remotes").join(format!("{name}.git"));
        git(
            &work,
            &[
                "push",
                "-q",
                &bare.to_string_lossy(),
                &format!("HEAD:refs/pull/{number}/head"),
            ],
        );
        let head = std::fs::read_to_string(bare.join(format!("refs/pull/{number}/head")));
        head.unwrap().trim().to_owned()
    }

    #[test]
    fn a_sync_fetches_the_heads_of_the_users_prs_into_the_mirror() {
        let (folders, root) = temp_folders("wiring-heads");
        let remotes = remotes(&root, &[CPYTHON]);
        // The user's own PR in the recordings.
        let commit = pull_request_on(&root, CPYTHON, 157_178);
        let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);

        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let _clone = clones(&wired, 1);
        let mirror =
            folders.mirror(&tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap());
        let fetched = mirror.join("refs/tsugiki/pull/157178/head");
        let deadline = Instant::now() + PLENTY;
        while !fetched.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(wired.stop());
        let head = std::fs::read_to_string(&fetched).unwrap_or_default();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(head.trim(), commit);
    }

    #[test]
    fn a_prs_files_tab_fetches_its_head_first_and_then_reads_its_diff_in_chunks() {
        let (folders, root) = temp_folders("wiring-files");
        let remotes = remotes(&root, &[CPYTHON]);
        let commit = pull_request_on(&root, CPYTHON, 1);
        // A PR of someone else's, at the commit on the remote, which no sync step reads.
        let pr = PullRequestFacts {
            number: 1,
            title: "gh-1: Fix it".to_owned(),
            state: PrState::Open,
            is_draft: false,
            author: Some(Login::new("some-author")),
            target_branch: "main".to_owned(),
            ci: None,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
            review_requests: vec![],
            latest_reviews: vec![],
            assignees: vec![],
            closing_issues: vec![],
            merge_commit: None,
            head: CommitSha::parse(&commit),
            off_default: None,
        };
        let name = tsugiki_core::repository::RepositoryName::parse(CPYTHON).unwrap();
        let store = tsugiki_core::store::Store::open(&folders.database()).unwrap();
        store.save_pull_requests(&name, vec![pr]).unwrap();
        store.close();
        let wired = Wired::start_cloning_from(Ok(folders.clone()), recordings(), remotes);
        let _ = wired.ask("add_repository", add_cpython()).unwrap();
        let _clone = clones(&wired, 1);
        let files = || {
            wired.ask(
                "pull_request_files",
                json!({ "repository": CPYTHON, "number": 1 }),
            )
        };
        let chunk_of = |head: &str, base: &str| json!({ "repository": CPYTHON, "number": 1, "head": head, "base": base, "start": 0, "end": 512 });
        let chunk = |head: &str, base: &str| {
            wired.ask(
                "pull_request_rows",
                json!({ "chunk": chunk_of(head, base) }),
            )
        };

        let fetching = files().unwrap();
        let heard = wired.heads.recv_timeout(PLENTY).unwrap();
        let ready = files().unwrap();
        let base = ready["base"].as_str().unwrap_or_default().to_owned();
        let rows = chunk(&commit, &base).unwrap();
        let spans = wired.ask(
            "pull_request_spans",
            json!({ "chunk": chunk_of(&commit, &base), "opened": [] }),
        );
        let lines_of = |start: u32, end: u32| {
            wired.ask(
                "pull_request_lines",
                json!({ "lines": { "repository": CPYTHON, "number": 1, "head": commit, "base": base, "file": 0, "start": start, "end": end } }),
            )
        };
        let lines = lines_of(1, 21);
        let found = find_in_pr_1(&wired, (&commit, &base), "pr");
        let from_0 = lines_of(0, 21).unwrap_err();
        let backwards = lines_of(21, 1).unwrap_err();
        let no_commit = chunk("main", &base).unwrap_err();
        let no_base = chunk(&commit, "main").unwrap_err();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(fetching, json!({ "state": "fetching" }));
        assert_eq!(heard, json!({ "repository": CPYTHON, "items": [1] }));
        let news = json!({
            "path": "NEWS",
            "old_path": null,
            "change": { "kind": "lines", "added": 1, "removed": 0 },
            "row": 0,
            "columns": 4,
            "generated": false,
            "gaps": [],
            "rest": null,
            "pairs": [{ "row": 2, "removed": 0, "added": 1 }]
        });
        assert_eq!(
            ready,
            json!({ "state": "ready", "head": commit, "base": base, "files": [news], "rows": 3, "news": [], "threads": { "kind": "shown", "threads": [] } })
        );
        assert!(CommitSha::parse(&base).is_some_and(|base| base.as_str() != commit));
        assert_eq!(
            rows,
            json!([
                {
                    "kind": "file",
                    "path": "NEWS",
                    "old_path": null,
                    "change": { "kind": "lines", "added": 1, "removed": 0 }
                },
                { "kind": "hunk", "text": "@@ -0,0 +1 @@" },
                { "kind": "added", "new": 1, "text": "PR 1" }
            ])
        );
        assert_eq!(no_commit["message"], "\"main\" isn't a commit.");
        assert_eq!(no_base["message"], "\"main\" isn't a commit.");
        // NEWS is in no language Tsugiki highlights.
        assert_eq!(spans.unwrap(), json!([[], [], []]));
        // The file has one line, read from the mirror.
        // A search finds the line too, whatever the case.
        let found_line = json!({ "matches": [{ "row": 2, "start": 0, "end": 2 }], "more": false });
        assert_eq!(
            (lines.unwrap(), found.unwrap()),
            (json!([{ "text": "PR 1", "spans": [] }]), found_line)
        );
        assert_eq!(from_0["message"], "Lines 0 to 21 aren't lines of a file.");
        assert_eq!(
            backwards["message"],
            "Lines 21 to 1 aren't lines of a file."
        );
    }
}
