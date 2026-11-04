//! The window's commands and events, set up on any Tauri runtime. `startup.rs` calls this on the real one.
//! The tests here call it on Tauri's mock one, so they check this wiring without a window.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tsugiki_core::error::Error;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::latest;
use tsugiki_core::session::Session;
use tsugiki_core::settings::{SettingsFiles, SettingsOwner};
use tsugiki_core::status::{Status, StatusBoard};

use crate::{StatusChanged, WorkbenchChanged, sync_home};

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
    // Two small files, read before the window exists so `settings` and `home` can always answer. A
    // `settings_path` on a slow drive would delay the window. Without folders the defaults apply, and the
    // session shows why. A thread of their own owns the files from here, so saving doesn't wait on GitHub.
    let settings = SettingsOwner::start(folders.clone().map(SettingsFiles::load));
    let status_handle = app.clone();
    let workbench_handle = app.clone();
    let board = StatusBoard::new(Box::new(move |status| {
        // Before the event: a window that starts listening too late for it then asks `status()`, and has to
        // get this status.
        latest_status.set(status);
        // A window still loading asks `status()` when it is done, so it doesn't need this event.
        log_if_unsent(StatusChanged(status.clone()).emit(&status_handle));
    }))
    .with_workbench_changes(Box::new(move |repository| {
        let repository = repository.clone();
        log_if_unsent(WorkbenchChanged { repository }.emit(&workbench_handle));
    }));
    let session = Session::start(board, folders, access, refresh);
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
            let workbenches = hear(&app, "workbench-changed");
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
        json!({ "name": CPYTHON })
    }

    #[test]
    fn with_no_folders_every_command_that_needs_them_says_so_in_the_cores_form() {
        let wired = Wired::start(Err(no_folders()), no_network());

        let checking = wired.ask("check_repository", json!({ "input": "a/b" }));
        let adding = wired.ask("add_repository", json!({ "name": "a/b" }));
        let reading = wired.ask("workbench", json!({ "repository": "a/b" }));
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
        let read = wired.ask("workbench", json!({ "repository": CPYTHON }));
        let waited = started.elapsed();
        drop(release);
        drop(wired.stop());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap()["screen"], "repository");
        assert_eq!(read.unwrap()["rows"], json!([]));
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

    /// The recordings hold 22 PRs of real CPython work, and 4 of them are the recording user's own.
    fn assert_is_the_recorded_work(workbench: &Value) {
        let rows = workbench["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 22);
        let with_reason = |reason: &str| {
            rows.iter()
                .filter(|row| row["reasons"].as_array().unwrap().contains(&json!(reason)))
                .count()
        };
        assert_eq!(with_reason("own_pr"), 4);
        assert!(with_reason("review_request") > 0);
    }

    /// Waits for the session to save what it found, and returns the Workbench after that.
    fn workbench_after_sync(wired: &Wired) -> Value {
        let changed = wired.workbenches.recv_timeout(PLENTY).unwrap();
        assert_eq!(changed, json!({ "repository": CPYTHON }));
        wired
            .ask("workbench", json!({ "repository": CPYTHON }))
            .unwrap()
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
