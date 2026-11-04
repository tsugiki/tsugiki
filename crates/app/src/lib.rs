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
use tsugiki_core::session::RepositoryChecks;
use tsugiki_core::settings::{LoadedSettings, SettingsOwner};
use tsugiki_core::status::Status;

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
    checks: &RepositoryChecks,
) -> Result<RepositoryCheck, Error> {
    checks.check(RepositoryName::parse(input)?)
}

fn add_named_repository(name: &str, owner: &SettingsOwner) -> Result<Home, Error> {
    Ok(owner.add_repository(RepositoryName::parse(name)?)?.home)
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            about,
            status,
            settings,
            home,
            slow_commands::check_repository,
            slow_commands::add_repository
        ])
        .events(collect_events![StatusChanged])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tauri::Manager;
    use tauri::test::mock_app;
    use tsugiki_core::error::ErrorKind;
    use tsugiki_core::folders::Folders;
    use tsugiki_core::github::GitHubAccess;
    use tsugiki_core::github::live::{Https, run_gh};
    use tsugiki_core::latest::channel;
    use tsugiki_core::repository::{Conventions, RepositorySettings};
    use tsugiki_core::session::Session;
    use tsugiki_core::settings::SettingsFiles;
    use tsugiki_core::status::StatusBoard;

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

        let added = add_named_repository("https://github.com/python/cpython", &app.state());
        let after = home(app.state());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), cpython());
        assert_eq!(after, cpython());
    }

    #[test]
    fn a_name_that_isnt_one_is_refused_before_anything_else_happens() {
        let owner = SettingsOwner::start(Err(no_folders()));
        let session = ended_session();

        let adding = add_named_repository("nonsense", &owner).unwrap_err();
        let checking =
            check_typed_repository("nonsense", &session.repository_checks()).unwrap_err();

        assert_eq!(adding.kind, ErrorKind::NotFound);
        assert_eq!(checking.kind, ErrorKind::NotFound);
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
            .manage(session.repository_checks())
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
    fn a_check_goes_to_the_session() {
        let session = ended_session();

        let answer =
            check_typed_repository("python/cpython", &session.repository_checks()).unwrap_err();
        session.stop(Duration::from_secs(10));

        assert_eq!(answer.kind, ErrorKind::Offline);
        assert!(answer.message.contains("isn't connected"), "{answer:?}");
    }
}
