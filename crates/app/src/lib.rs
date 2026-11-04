//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

#![forbid(unsafe_code)]

mod slow_commands;
mod startup;

use serde::Serialize;
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::error::Error;
use tsugiki_core::latest::LatestReader;
use tsugiki_core::repository::{Home, RepositoryCheck, RepositoryName};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::{LoadedSettings, SettingsOwner};
use tsugiki_core::status::Status;
use tsugiki_core::store::StoreReader;
use tsugiki_core::workbench::Workbench;

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about()
}

type LatestStatus = LatestReader<Status>;

/// The whole app-wide state, for a window that has just loaded (ADR 0008).
#[tauri::command]
#[specta::specta]
fn status(latest: State<'_, LatestStatus>) -> Status {
    latest.get()
}

/// The settings Tsugiki last read, and what was wrong with their files (ADR 0014).
#[tauri::command]
#[specta::specta]
fn settings(owner: State<'_, SettingsOwner>) -> LoadedSettings {
    owner.view().loaded
}

/// What the window opens on.
#[tauri::command]
#[specta::specta]
fn home(owner: State<'_, SettingsOwner>) -> Home {
    owner.view().home
}

fn check_typed_repository(
    input: &str,
    requests: &SessionRequests,
) -> Result<RepositoryCheck, Error> {
    requests.check(RepositoryName::parse(input)?)
}

/// Adds the Repository and starts its first sync (ADR 0015).
fn add_named_repository(
    name: &str,
    owner: &SettingsOwner,
    requests: &SessionRequests,
) -> Result<Home, Error> {
    let home = owner.add_repository(RepositoryName::parse(name)?)?.home;
    sync_home(&home, requests);
    Ok(home)
}

/// Loads the user's work in the Repository the window opens on, under the name the window knows it by, so the
/// window recognises the changes.
fn sync_home(home: &Home, requests: &SessionRequests) {
    match home {
        Home::Repository { repository } => requests.sync(repository.name.clone()),
        Home::Onboarding { .. } => {}
    }
}

fn read_workbench(repository: &str, reader: &StoreReader) -> Result<Workbench, Error> {
    Workbench::read(reader, &RepositoryName::parse(repository)?)
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Sent when a Repository's stored facts change, so a window showing its Workbench asks again (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct WorkbenchChanged {
    repository: RepositoryName,
}

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            about,
            status,
            settings,
            home,
            slow_commands::check_repository,
            slow_commands::add_repository,
            slow_commands::workbench
        ])
        .events(collect_events![StatusChanged, WorkbenchChanged])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::mpsc::{Receiver, channel as mpsc_channel};
    use std::time::Duration;
    use tauri::Manager;
    use tauri::test::mock_app;
    use tsugiki_core::conventions::Conventions;
    use tsugiki_core::error::ErrorKind;
    use tsugiki_core::folders::Folders;
    use tsugiki_core::github::GitHubAccess;
    use tsugiki_core::github::live::{Https, run_gh};
    use tsugiki_core::github::replay::Replayer;
    use tsugiki_core::latest::channel;
    use tsugiki_core::repository::RepositorySettings;
    use tsugiki_core::session::Session;
    use tsugiki_core::settings::SettingsFiles;
    use tsugiki_core::status::StatusBoard;
    use tsugiki_core::token::GhOutput;
    use tsugiki_core::workbench::WorkReason;

    fn temp_folders(name: &str) -> (Folders, std::path::PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../.temp/app-{name}"));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).unwrap();
        let folders = Folders {
            data: root.clone(),
            settings: root.clone(),
        };
        (folders, root)
    }

    fn cpython() -> Home {
        Home::Repository {
            repository: RepositorySettings {
                name: RepositoryName::parse("python/cpython").unwrap(),
                conventions: Conventions::Cpython,
            },
        }
    }

    #[test]
    fn about_answers_with_what_the_core_says() {
        assert_eq!(about(), tsugiki_core::about());
    }

    #[test]
    fn status_says_connecting_until_the_session_says_otherwise() {
        let app = mock_app();
        let (_writer, reader) = channel(Status::default());
        assert!(app.manage(reader));
        assert_eq!(status(app.state()), Status::Connecting);
    }

    #[test]
    fn status_answers_with_the_latest_status() {
        let app = mock_app();
        let (mut writer, reader) = channel(Status::default());
        assert!(app.manage(reader));
        writer.set(&Status::Offline);
        assert_eq!(status(app.state()), Status::Offline);
    }

    #[test]
    fn settings_says_what_was_wrong_with_the_files() {
        let (folders, root) = temp_folders("problems");
        std::fs::write(
            folders.settings_file(),
            "[status_bar
",
        )
        .unwrap();
        let app = mock_app();
        assert!(app.manage(SettingsOwner::start(Ok(SettingsFiles::load(folders)))));
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(settings(app.state()).problems.len(), 1);
    }

    fn no_folders() -> Error {
        Error::new(ErrorKind::Internal, "no folders in this test")
    }

    #[test]
    fn a_repository_added_is_what_the_window_opens_on() {
        let (folders, root) = temp_folders("add");
        let app = mock_app();
        assert!(app.manage(SettingsOwner::start(Ok(SettingsFiles::load(folders)))));
        assert_eq!(home(app.state()), Home::default());

        let session = ended_session();
        let added = add_named_repository(
            "https://github.com/python/cpython",
            &app.state(),
            &session.requests(),
        );
        let after = home(app.state());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), cpython());
        assert_eq!(after, cpython());
    }

    #[test]
    fn a_name_that_isnt_one_is_refused_before_anything_else_happens() {
        let owner = SettingsOwner::start(Err(no_folders()));
        let session = ended_session();

        let adding = add_named_repository("nonsense", &owner, &session.requests()).unwrap_err();
        let checking = check_typed_repository("nonsense", &session.requests()).unwrap_err();
        let reading = read_workbench("nonsense", &session.reader()).unwrap_err();

        assert_eq!(adding.kind, ErrorKind::NotFound);
        assert_eq!(checking.kind, ErrorKind::NotFound);
        assert_eq!(reading.kind, ErrorKind::NotFound);
    }

    /// A session with no folders, which ends as soon as it starts, without asking anything of `Https`.
    fn ended_session() -> Session {
        let access = GitHubAccess {
            transport: Box::new(Https::new()),
            run_gh,
        };
        let board = StatusBoard::new(Box::new(|_| {}));
        Session::start(board, Err(no_folders()), access, Duration::from_secs(3600))
    }

    /// What the webview gets back for `command`, through Tauri's own handling of its arguments and threads.
    fn through_ipc(command: &str, arguments: serde_json::Value) -> serde_json::Value {
        let session = ended_session();
        let app = tauri::test::mock_builder()
            .invoke_handler(specta_builder().invoke_handler())
            .manage(session.requests())
            .manage(session.reader())
            .manage(SettingsOwner::start(Err(no_folders())))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        tauri::test::get_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: command.to_owned(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: webview.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(arguments),
                headers: tauri::http::HeaderMap::default(),
                invoke_key: tauri::test::INVOKE_KEY.to_owned(),
            },
        )
        .unwrap_err()
    }

    #[test]
    fn the_webview_can_ask_for_a_check_and_gets_the_failure_in_the_cores_form() {
        let answer = through_ipc("check_repository", serde_json::json!({ "input": "a/b" }));
        assert_eq!(answer["kind"], "offline");
        assert!(
            answer["message"]
                .as_str()
                .unwrap()
                .contains("isn't connected")
        );
    }

    #[test]
    fn the_webview_can_ask_for_an_add_and_gets_the_failure_in_the_cores_form() {
        let answer = through_ipc("add_repository", serde_json::json!({ "name": "a/b" }));
        assert_eq!(answer["kind"], "internal");
        assert_eq!(answer["message"], no_folders().message);
    }

    #[test]
    fn the_webview_can_ask_for_a_workbench_and_gets_the_failure_in_the_cores_form() {
        let answer = through_ipc("workbench", serde_json::json!({ "repository": "a/b" }));
        assert_eq!(answer["kind"], "internal");
    }

    fn signed_in_gh(_: &[&str]) -> std::io::Result<GhOutput> {
        Ok(GhOutput {
            success: true,
            stdout: "gho_example".to_owned(),
        })
    }

    /// A session that answers from the recordings of real GitHub, and says when a Workbench changes.
    fn recorded_session(folders: Folders) -> (Session, Receiver<RepositoryName>) {
        let recordings =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/fixtures/github");
        let access = GitHubAccess {
            transport: Box::new(Replayer::new(recordings)),
            run_gh: signed_in_gh,
        };
        let (changed, changes) = mpsc_channel();
        let board = StatusBoard::new(Box::new(|_| {})).with_workbench_changes(Box::new(
            move |repository: &RepositoryName| drop(changed.send(repository.clone())),
        ));
        let session = Session::start(board, Ok(folders), access, Duration::from_secs(3600));
        (session, changes)
    }

    /// Waits for the session to save what it found, and returns the Workbench after that.
    fn workbench_after_sync(session: &Session, changes: &Receiver<RepositoryName>) -> Workbench {
        let name = changes.recv_timeout(Duration::from_secs(60)).unwrap();
        assert_eq!(name, RepositoryName::parse("python/cpython").unwrap());
        read_workbench("python/cpython", &session.reader()).unwrap()
    }

    /// The recordings hold 22 PRs of real CPython work, and 4 of them are the recording user's own.
    fn assert_is_the_recorded_work(workbench: &Workbench) {
        assert_eq!(workbench.rows.len(), 22);
        let own = workbench
            .rows
            .iter()
            .filter(|row| row.reasons.contains(&WorkReason::OwnPr));
        assert_eq!(own.count(), 4);
        assert!(
            workbench
                .rows
                .iter()
                .any(|row| row.reasons.contains(&WorkReason::ReviewRequest))
        );
    }

    #[test]
    fn adding_a_repository_brings_in_what_waits_on_the_user() {
        let (folders, root) = temp_folders("first-sync");
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
        let (session, changes) = recorded_session(folders);

        let added = add_named_repository("python/cpython", &owner, &session.requests());
        let workbench = workbench_after_sync(&session, &changes);
        session.stop(Duration::from_secs(10));
        drop(session);
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), cpython());
        assert_is_the_recorded_work(&workbench);
    }

    #[test]
    fn the_app_syncs_the_repository_it_reopens() {
        let (folders, root) = temp_folders("reopen");
        let owner = SettingsOwner::start(Ok(SettingsFiles::load(folders.clone())));
        drop(owner.add_repository(RepositoryName::parse("python/cpython").unwrap()));
        let (session, changes) = recorded_session(folders);

        sync_home(&owner.view().home, &session.requests());
        let workbench = workbench_after_sync(&session, &changes);
        session.stop(Duration::from_secs(10));
        drop(session);
        std::fs::remove_dir_all(root).unwrap();

        assert_is_the_recorded_work(&workbench);
    }

    #[test]
    fn with_no_repository_the_app_has_nothing_to_sync() {
        let (folders, root) = temp_folders("nothing-to-sync");
        let (session, changes) = recorded_session(folders);

        sync_home(&Home::default(), &session.requests());
        session.stop(Duration::from_secs(10));
        drop(session);
        std::fs::remove_dir_all(root).unwrap();

        assert!(changes.try_recv().is_err());
    }

    #[test]
    fn a_check_goes_to_the_session() {
        let session = ended_session();

        let answer = check_typed_repository("python/cpython", &session.requests()).unwrap_err();
        session.stop(Duration::from_secs(10));

        assert_eq!(answer.kind, ErrorKind::Offline);
        assert!(answer.message.contains("isn't connected"), "{answer:?}");
    }
}
