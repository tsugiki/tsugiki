//! The window's commands and events, set up on any Tauri runtime. `startup.rs` calls this on the real one.
//! The tests here call it on Tauri's mock one, so they check this wiring without a window.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tsugiki_core::error::Error;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::latest;
use tsugiki_core::session::{Intervals, Session};
use tsugiki_core::settings::{SettingsFiles, SettingsOwner};
use tsugiki_core::status::{Status, StatusBoard};

use crate::{Changed, LastSyncChanged, StatusChanged, SyncActivityChanged, sync_home};

/// How often the open Repository refreshes: a refresh costs a few points, and this is about 60 points an
/// hour, out of 5,000 (ADR 0015).
const REPOSITORY_REFRESH: Duration = Duration::from_secs(180);

/// Reads the settings, starts the session and gives the commands what they ask. Events need `mount_events` to
/// have run on `app`.
pub(crate) fn connect_core<R: Runtime>(
    app: &AppHandle<R>,
    folders: Result<Folders, Error>,
    access: GitHubAccess,
    refresh: Duration,
) -> Session {
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
    // Two small files, read before the window exists so `settings` and `home` can always answer. A
    // `settings_path` on a slow drive would delay the window. Without folders the defaults apply, and the
    // session shows why. A thread of their own owns the files from here, so saving doesn't wait on GitHub.
    let settings = SettingsOwner::start(folders.clone().map(SettingsFiles::load));
    let status_handle = app.clone();
    let changes_handle = app.clone();
    let last_sync_handle = app.clone();
    let activity_handle = app.clone();
    let board = StatusBoard::new(Box::new(move |status| {
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
    }));
    let intervals = Intervals {
        status: refresh,
        repository: REPOSITORY_REFRESH,
    };
    let session = Session::start(board, folders, access, intervals);
    let requests = session.requests();
    sync_home(&settings.view().home, &requests);
    assert!(app.manage(settings), "only this manages the settings");
    assert!(app.manage(requests), "only this manages the requests");
    let reader = session.reader();
    assert!(app.manage(reader), "only this manages the reader");
    session
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
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::Instant;
    use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
    use tauri::{App, Listener, WebviewWindow};
    use tsugiki_core::github::replay::Replayer;
    use tsugiki_core::github::{Reply, Request, Transport, Unreachable};
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
            Err(Unreachable)
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
            Err(Unreachable)
        }
    }

    /// The app as `startup.rs` wires it, on Tauri's mock runtime with a webview to send commands from. It
    /// listens before the session starts, so it hears every event.
    struct Wired {
        session: Session,
        webview: WebviewWindow<MockRuntime>,
        statuses: Receiver<Value>,
        workbenches: Receiver<Value>,
        last_syncs: Receiver<Value>,
        activities: Receiver<Value>,
        _app: App<MockRuntime>,
    }

    impl Wired {
        fn start(folders: Result<Folders, Error>, access: GitHubAccess) -> Self {
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
            let session = connect_core(app.handle(), folders, access, Duration::from_secs(3600));
            let webview =
                tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
                    .build()
                    .unwrap();
            Self {
                session,
                webview,
                statuses,
                workbenches,
                last_syncs,
                activities,
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
            self.session.stop(PLENTY);
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
        json!({ "name": CPYTHON, "access": "other" })
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
    fn a_name_that_isnt_a_repository_opens_nothing() {
        let (folders, root) = temp_folders("wired-open-bad-name");
        let wired = Wired::start(Ok(folders), no_network());
        let bad = json!({ "repository": "not a name", "number": 7 });

        let open = wired.ask("open_issue", bad.clone()).unwrap_err();
        let fetch = wired.ask("fetch_issue", bad.clone()).unwrap_err();
        let read = wired.ask("issue", bad).unwrap_err();
        let requests = wired.session.requests();
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
        assert!(page.is_err());
    }

    #[test]
    fn opening_an_item_on_github_shows_its_page_and_then_moves_its_last_seen() {
        let (folders, root) = temp_folders("wired-open-on-github");
        let wired = Wired::start(Ok(folders), no_network());
        let requests = wired.session.requests();
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
        let requests = wired.session.requests();

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

    /// The recordings hold 22 PRs of real CPython work, and 4 of them are the recording user's own. They name
    /// 22 Issues: #154019 fixes two by itself, and #156423 and #156419 fix one between them. The Filters'
    /// search reaches 2 more that no PR names.
    fn assert_is_the_recorded_work(workbench: &Value) {
        let items = listed(workbench);
        assert_eq!(items.len(), 24);
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
        assert_eq!(with_reason("own_pr"), 4);
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
        // The search found 8 items, and #156423 and #156419 share a row with the Issue they fix.
        assert_eq!(listed(&selected).len(), 6);
    }

    #[test]
    fn a_maintained_repository_starts_with_its_own_filters() {
        let (folders, root) = temp_folders("wired-maintained");
        let wired = Wired::start(Ok(folders.clone()), no_network());
        let adding = json!({ "name": CPYTHON, "access": "maintained" });

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
            parts.push(heard["part"].as_str().unwrap().to_owned());
        };
        parts.dedup();
        let asked = wired.ask("sync_activity", json!({})).unwrap();
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(parts, ["refresh", "your_work", "window", "trees"]);
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
}
