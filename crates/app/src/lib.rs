//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

#![forbid(unsafe_code)]

mod slow_commands;
mod startup;
mod wiring;

use serde::Serialize;
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::error::Error;
use tsugiki_core::latest::LatestReader;
use tsugiki_core::repository::{Home, RepositoryAccess, RepositoryCheck, RepositoryName};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::{LoadedSettings, SettingsOwner};
use tsugiki_core::status::{Change, LastSync, Status, SyncActivity};
use tsugiki_core::store::StoreReader;
use tsugiki_core::workbench::{Workbench, WorkbenchView};

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

type LatestLastSync = LatestReader<Option<LastSync>>;

/// When the open Repository last finished a sync, for a window that has just loaded. A window asking before
/// the core knows gets nothing, and the core's event then tells it.
#[tauri::command]
#[specta::specta]
fn last_sync(latest: State<'_, LatestLastSync>) -> Option<LastSync> {
    latest.get()
}

type LatestSyncActivity = LatestReader<Option<SyncActivity>>;

/// What the queue of syncs is doing, for a window that has just loaded.
#[tauri::command]
#[specta::specta]
fn sync_activity(latest: State<'_, LatestSyncActivity>) -> Option<SyncActivity> {
    latest.get()
}

/// Refreshes the open Repository now, and returns at once.
#[tauri::command]
#[specta::specta]
fn refresh(requests: State<'_, SessionRequests>) {
    requests.refresh();
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
    access: RepositoryAccess,
    owner: &SettingsOwner,
    requests: &SessionRequests,
) -> Result<Home, Error> {
    let home = owner
        .add_repository(RepositoryName::parse(name)?, access)?
        .home;
    sync_home(&home, requests);
    Ok(home)
}

/// Loads the user's work in the Repository the window opens on, under the name the window knows it by, so the
/// window recognises the changes.
fn sync_home(home: &Home, requests: &SessionRequests) {
    match home {
        Home::Repository { repository } => requests.sync(repository.clone()),
        Home::Onboarding { .. } => {}
    }
}

fn read_workbench(
    repository: &str,
    view: &WorkbenchView,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Workbench, Error> {
    let name = RepositoryName::parse(repository)?;
    let repository = owner.view().loaded.settings.repository(name);
    Workbench::read(reader, &repository, view)
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Sent when a Repository's stored facts change, so each screen showing them asks again (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct Changed(Change);

/// Sent when the open Repository's last sync changes, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct LastSyncChanged(LastSync);

/// Sent when what the queue of syncs is doing changes, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct SyncActivityChanged(Option<SyncActivity>);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            about,
            status,
            last_sync,
            sync_activity,
            refresh,
            settings,
            home,
            slow_commands::check_repository,
            slow_commands::add_repository,
            slow_commands::workbench
        ])
        .events(collect_events![
            StatusChanged,
            Changed,
            LastSyncChanged,
            SyncActivityChanged
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::Duration;
    use tauri::Manager;
    use tauri::test::mock_app;
    use tsugiki_core::conventions::Conventions;
    use tsugiki_core::error::ErrorKind;
    use tsugiki_core::filters::Filter;
    use tsugiki_core::folders::Folders;
    use tsugiki_core::github::GitHubAccess;
    use tsugiki_core::github::live::{Https, run_gh};
    use tsugiki_core::latest::channel;
    use tsugiki_core::repository::RepositorySettings;
    use tsugiki_core::session::{Intervals, Session};
    use tsugiki_core::settings::SettingsFiles;
    use tsugiki_core::status::StatusBoard;

    pub(crate) fn temp_folders(name: &str) -> (Folders, std::path::PathBuf) {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../.temp/app-{name}"));
        drop(std::fs::remove_dir_all(&root));
        std::fs::create_dir_all(&root).unwrap();
        let folders = Folders {
            data: root.clone(),
            settings: root.clone(),
        };
        (folders, root)
    }

    pub(crate) fn cpython() -> Home {
        Home::Repository {
            repository: RepositorySettings {
                name: RepositoryName::parse("python/cpython").unwrap(),
                conventions: Conventions::Cpython,
                filters: Filter::starters(RepositoryAccess::Other),
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

    pub(crate) fn no_folders() -> Error {
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
            RepositoryAccess::Other,
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

        let adding = add_named_repository(
            "nonsense",
            RepositoryAccess::Other,
            &owner,
            &session.requests(),
        )
        .unwrap_err();
        let checking = check_typed_repository("nonsense", &session.requests()).unwrap_err();
        let reading = read_workbench("nonsense", &WorkbenchView::Whole, &owner, &session.reader())
            .unwrap_err();

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
        let hourly = Duration::from_secs(3600);
        let intervals = Intervals {
            status: hourly,
            repository: hourly,
        };
        Session::start(board, Err(no_folders()), access, intervals)
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
