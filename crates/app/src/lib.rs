//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

#![forbid(unsafe_code)]

mod slow_commands;
mod startup;

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::repository::{Home, RepositoryCheck, RepositoryName};
use tsugiki_core::session::RepositoryChecks;
use tsugiki_core::settings::{LoadedSettings, SettingsFiles};
use tsugiki_core::status::Status;

const NO_FOLDERS: &str = "Tsugiki has no folder to save its settings in. The status bar says why.";

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about()
}

/// A read-only copy of the session's status. It lets `status` answer while the session waits on GitHub, since
/// the window doesn't wait on the network (ADR 0017).
#[derive(Default)]
struct LatestStatus(Mutex<Status>);

impl LatestStatus {
    fn get(&self) -> Status {
        self.lock().clone()
    }

    fn set(&self, status: &Status) {
        self.lock().clone_from(status);
    }

    /// Only this type holds the lock, and it calls out to nothing while it does, so poisoning should never
    /// happen. If it does, the status is still whole, because `set` only ever replaces it.
    fn lock(&self) -> MutexGuard<'_, Status> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The whole app-wide state, for a window that has just loaded (ADR 0008).
#[tauri::command]
#[specta::specta]
fn status(latest: State<'_, LatestStatus>) -> Status {
    latest.get()
}

/// The settings files, which own what they say. `None` means Tsugiki found no folders: the defaults apply,
/// and the session shows why.
struct OwnedSettings(Mutex<Option<SettingsFiles>>);

impl OwnedSettings {
    fn new(files: Option<SettingsFiles>) -> Self {
        Self(Mutex::new(files))
    }

    /// Only this type holds the lock. A panic while saving would leave the files to say what they say, and
    /// the next read or save works from those.
    fn lock(&self) -> MutexGuard<'_, Option<SettingsFiles>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The settings Tsugiki last read, and what was wrong with their files (ADR 0014).
#[tauri::command]
#[specta::specta]
fn settings(owned: State<'_, OwnedSettings>) -> LoadedSettings {
    owned
        .lock()
        .as_ref()
        .map(|files| files.loaded().clone())
        .unwrap_or_default()
}

/// What the window opens on.
#[tauri::command]
#[specta::specta]
fn home(owned: State<'_, OwnedSettings>) -> Home {
    owned
        .lock()
        .as_ref()
        .map(SettingsFiles::home)
        .unwrap_or_default()
}

fn check_typed_repository(
    input: &str,
    checks: &RepositoryChecks,
) -> Result<RepositoryCheck, Error> {
    checks.check(RepositoryName::parse(input)?)
}

fn add_named_repository(name: &str, owned: &OwnedSettings) -> Result<Home, Error> {
    let name = RepositoryName::parse(name)?;
    owned
        .lock()
        .as_mut()
        .ok_or_else(|| Error::new(ErrorKind::Internal, NO_FOLDERS))?
        .add_repository(&name)
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
    use tsugiki_core::folders::Folders;
    use tsugiki_core::github::GitHubAccess;
    use tsugiki_core::github::live::{Https, run_gh};
    use tsugiki_core::repository::{Conventions, RepositorySettings};
    use tsugiki_core::session::Session;
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
        assert!(app.manage(LatestStatus::default()));
        assert_eq!(status(app.state()), Status::Connecting);
    }

    #[test]
    fn status_answers_with_the_latest_status() {
        let app = mock_app();
        assert!(app.manage(LatestStatus::default()));
        app.state::<LatestStatus>().set(&Status::Offline);
        assert_eq!(status(app.state()), Status::Offline);
    }

    #[test]
    fn settings_answers_with_what_the_files_said() {
        let (folders, root) = temp_folders("settings");
        std::fs::write(folders.settings_file(), "[status_bar\n").unwrap();
        let app = mock_app();
        assert!(app.manage(OwnedSettings::new(Some(SettingsFiles::load(folders)))));

        let loaded = settings(app.state());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(loaded.problems.len(), 1);
    }

    #[test]
    fn without_folders_the_window_opens_on_the_defaults_and_adding_says_why_not() {
        let app = mock_app();
        assert!(app.manage(OwnedSettings::new(None)));

        assert_eq!(settings(app.state()), LoadedSettings::default());
        assert_eq!(home(app.state()), Home::default());
        let refused = add_named_repository("python/cpython", &app.state()).unwrap_err();
        assert_eq!(refused.message, NO_FOLDERS);
    }

    #[test]
    fn a_repository_added_is_what_the_window_opens_on() {
        let (folders, root) = temp_folders("add");
        let app = mock_app();
        assert!(app.manage(OwnedSettings::new(Some(SettingsFiles::load(folders)))));
        assert_eq!(home(app.state()), Home::default());

        let added = add_named_repository("https://github.com/python/cpython", &app.state());
        let after = home(app.state());
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), cpython());
        assert_eq!(after, cpython());
    }

    #[test]
    fn a_name_that_isnt_one_is_refused_before_anything_else_happens() {
        let app = mock_app();
        assert!(app.manage(OwnedSettings::new(None)));
        let session = ended_session();
        assert!(app.manage(session.repository_checks()));

        let adding = add_named_repository("nonsense", &app.state()).unwrap_err();
        let checking = check_typed_repository("nonsense", &app.state()).unwrap_err();

        assert_eq!(adding.kind, ErrorKind::NotFound);
        assert_eq!(checking.kind, ErrorKind::NotFound);
    }

    /// A session with no folders, which ends as soon as it starts, without asking anything of `Https`.
    fn ended_session() -> Session {
        let access = GitHubAccess {
            transport: Box::new(Https::new()),
            run_gh,
        };
        let no_folders = Err(Error::new(ErrorKind::Internal, "no folders in this test"));
        let board = StatusBoard::new(Box::new(|_| {}));
        Session::start(board, no_folders, access, Duration::from_secs(3600))
    }

    /// What the webview gets back for `command`, through Tauri's own handling of its arguments and threads.
    fn through_ipc(command: &str, arguments: serde_json::Value) -> serde_json::Value {
        let session = ended_session();
        let app = tauri::test::mock_builder()
            .invoke_handler(specta_builder().invoke_handler())
            .manage(OwnedSettings::new(None))
            .manage(session.repository_checks())
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
        assert_eq!(answer["message"], NO_FOLDERS);
    }

    #[test]
    fn a_check_goes_to_the_session() {
        let app = mock_app();
        let session = ended_session();
        assert!(app.manage(session.repository_checks()));

        let answer = check_typed_repository("python/cpython", &app.state()).unwrap_err();
        session.stop(Duration::from_secs(10));

        assert_eq!(answer.kind, ErrorKind::Offline);
        assert!(answer.message.contains("isn't connected"), "{answer:?}");
    }

    #[test]
    fn a_panic_while_holding_the_lock_doesnt_stop_the_status_working() {
        let latest = LatestStatus::default();
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| {
                let _guard = latest.0.lock();
                panic!("a panic while holding the lock");
            });
            assert!(holder.join().is_err());
        });
        assert!(latest.0.is_poisoned());

        assert_eq!(latest.get(), Status::Connecting);
        latest.set(&Status::Offline);
        assert_eq!(latest.get(), Status::Offline);
    }
}
