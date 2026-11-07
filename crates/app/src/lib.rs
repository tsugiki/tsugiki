//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

#![forbid(unsafe_code)]

mod navigation;
mod slow_commands;
mod startup;
mod wiring;

use std::time::Instant;

use serde::{Deserialize, Serialize};
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::diff::{DiffRow, Diffs, PrAt};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::folders::Folders;
use tsugiki_core::github::CommitSha;
use tsugiki_core::latest::LatestReader;
use tsugiki_core::measurement::Measurement;
use tsugiki_core::mirror::{Cloning, MirrorRequests};
use tsugiki_core::misses::Miss;
use tsugiki_core::repository::{Home, RepositoryAccess, RepositoryCheck, RepositoryName};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::{LoadedSettings, SettingsOwner};
use tsugiki_core::status::{Change, LastSync, Status, SyncActivity};
use tsugiki_core::store::StoreReader;
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_files::{DiffSources, PrFiles};
use tsugiki_core::workbench::pr_view::PrView;
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

type LatestCloning = LatestReader<Option<Cloning>>;

/// The clone of a mirror under way, for a window that has just loaded.
#[tauri::command]
#[specta::specta]
fn cloning(latest: State<'_, LatestCloning>) -> Option<Cloning> {
    latest.get()
}

/// Refreshes the open Repository now, and returns at once.
#[tauri::command]
#[specta::specta]
fn refresh(requests: State<'_, SessionRequests>) {
    requests.refresh();
}

/// Tells the core the user opened an Issue, which moves its Last seen and reads what it says from GitHub, and
/// returns at once.
#[tauri::command]
#[specta::specta]
fn open_issue(
    repository: &str,
    number: u32,
    requests: State<'_, SessionRequests>,
) -> Result<(), Error> {
    requests.open_issue(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Tells the core the user opened a PR, which moves its Last seen and reads its Conversation from GitHub, and
/// returns at once.
#[tauri::command]
#[specta::specta]
fn open_pull_request(
    repository: &str,
    number: u32,
    requests: State<'_, SessionRequests>,
) -> Result<(), Error> {
    requests.open_pull_request(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Asks the core to read a PR's Conversation on screen now, and returns at once (ADR 0015).
#[tauri::command]
#[specta::specta]
fn fetch_pull_request(
    repository: &str,
    number: u32,
    requests: State<'_, SessionRequests>,
) -> Result<(), Error> {
    requests.fetch_pull_request(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Asks the core to read what an Issue on screen says now, and returns at once (ADR 0015).
#[tauri::command]
#[specta::specta]
fn fetch_issue(
    repository: &str,
    number: u32,
    requests: State<'_, SessionRequests>,
) -> Result<(), Error> {
    requests.fetch_issue(RepositoryName::parse(repository)?, number);
    Ok(())
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

/// The moment Tsugiki's code started running, for the performance HUD's start-up time (ADR 0017). The
/// process itself started a moment earlier, but Rust has no portable way to ask the OS when.
pub(crate) struct Started(Instant);

impl Started {
    pub(crate) fn now() -> Self {
        Self(Instant::now())
    }
}

/// The milliseconds since Tsugiki started running.
#[tauri::command]
#[specta::specta]
fn time_running(started: State<'_, Started>) -> f64 {
    started.0.elapsed().as_secs_f64() * 1000.0
}

/// Writes what the performance HUD measured to `tsugiki.log`, and says the summary line it wrote.
#[tauri::command]
#[specta::specta]
fn record_measurement(measurement: Measurement) -> String {
    let summary = measurement.summary();
    log::info!("{summary}");
    for line in measurement.long_frame_lines() {
        log::info!("{line}");
    }
    summary
}

/// Writes a budget the window missed to `tsugiki.log`, and says the line it wrote.
#[tauri::command]
#[specta::specta]
fn record_miss(miss: Miss) -> String {
    let line = miss.line();
    log::info!("{line}");
    line
}

fn check_typed_repository(
    input: &str,
    requests: &SessionRequests,
) -> Result<RepositoryCheck, Error> {
    requests.check(RepositoryName::parse(input)?)
}

/// Adds the Repository, and starts its first sync (ADR 0015) and its mirror's clone.
fn add_named_repository(
    name: &str,
    access: RepositoryAccess,
    owner: &SettingsOwner,
    work: &RepositoryWork,
) -> Result<Home, Error> {
    let home = owner
        .add_repository(RepositoryName::parse(name)?, access)?
        .home;
    work.start(&home);
    Ok(home)
}

/// The threads that work on Repositories: the session's sync queue, and the mirrors' one writer.
struct RepositoryWork {
    session: SessionRequests,
    mirrors: MirrorRequests,
}

impl RepositoryWork {
    /// Loads the user's work in the Repository the window opens on, under the name the window knows it by,
    /// so the window recognises the changes. Its mirror's clone goes ahead of the other Repositories', and
    /// neither waits on the other.
    fn start(&self, home: &Home) {
        match home {
            Home::Repository { repository } => {
                self.session.sync(repository.clone());
                self.mirrors.clone_mirror_next(repository.name.clone());
            }
            Home::Onboarding { .. } => {}
        }
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

fn read_issue(
    repository: &str,
    number: u32,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Option<IssueView>, Error> {
    let name = RepositoryName::parse(repository)?;
    let repository = owner.view().loaded.settings.repository(name);
    IssueView::read(reader, &repository, number)
}

fn read_pull_request(
    repository: &str,
    number: u32,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Option<PrView>, Error> {
    let name = RepositoryName::parse(repository)?;
    let repository = owner.view().loaded.settings.repository(name);
    PrView::read(reader, &repository, number)
}

/// What the Files tab reads besides the database: where the mirrors are, the diff on show, and the mirrors'
/// worker, which fetches a head a mirror lacks.
pub(crate) struct DiffWork {
    pub(crate) folders: Result<Folders, Error>,
    pub(crate) diffs: Diffs,
    pub(crate) mirrors: MirrorRequests,
}

impl DiffWork {
    fn sources<'a>(&'a self, reader: &'a StoreReader) -> Result<DiffSources<'a>, Error> {
        let folders = self.folders.as_ref().map_err(Clone::clone)?;
        Ok(DiffSources {
            reader,
            folders,
            diffs: &self.diffs,
            mirrors: &self.mirrors,
        })
    }
}

/// Rows of a PR's diff, as the window asks for them.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct DiffChunk {
    repository: String,
    number: u32,
    /// The commit the diff ends at, as the Files tab was told.
    head: String,
    start: u32,
    end: u32,
}

fn read_pull_request_files(
    repository: &str,
    number: u32,
    work: &DiffWork,
    reader: &StoreReader,
) -> Result<Option<PrFiles>, Error> {
    let name = RepositoryName::parse(repository)?;
    PrFiles::read(&work.sources(reader)?, &name, number)
}

fn read_pull_request_rows(
    chunk: DiffChunk,
    work: &DiffWork,
    reader: &StoreReader,
) -> Result<Vec<DiffRow>, Error> {
    let Some(head) = CommitSha::parse(&chunk.head) else {
        let message = format!("{:?} isn't a commit.", chunk.head);
        return Err(Error::new(ErrorKind::NotFound, &message));
    };
    let pr = PrAt {
        repository: RepositoryName::parse(&chunk.repository)?,
        number: chunk.number,
        head,
    };
    PrFiles::rows(&work.sources(reader)?, &pr, chunk.start..chunk.end)
}

/// Opens the page of `item` on GitHub with `show`, and then moves its Last seen, since the user has it in
/// front of them. If the browser didn't open, they haven't seen it.
fn open_github_page(
    repository: &str,
    item: GitHubItem,
    requests: &SessionRequests,
    show: impl FnOnce(&WebAddress) -> Result<(), Error>,
) -> Result<(), Error> {
    let name = RepositoryName::parse(repository)?;
    show(&WebAddress::of_item(&name, item))?;
    requests.seen(name, item.number());
    Ok(())
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

/// Sent when a mirror's clone moves on, and when it ends, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct CloningChanged(Option<Cloning>);

/// Sent when PR heads arrive in a Repository's mirror, so a Files tab waiting for one asks again.
#[derive(Clone, Serialize, Type, Event)]
struct HeadsFetched(Change);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            about,
            status,
            last_sync,
            sync_activity,
            cloning,
            refresh,
            open_issue,
            fetch_issue,
            open_pull_request,
            fetch_pull_request,
            settings,
            home,
            time_running,
            record_measurement,
            record_miss,
            slow_commands::check_repository,
            slow_commands::add_repository,
            slow_commands::workbench,
            slow_commands::issue,
            slow_commands::pull_request,
            slow_commands::pull_request_files,
            slow_commands::pull_request_rows,
            slow_commands::open_on_github,
            slow_commands::open_link
        ])
        .events(collect_events![
            StatusChanged,
            Changed,
            LastSyncChanged,
            SyncActivityChanged,
            CloningChanged,
            HeadsFetched
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
    use tsugiki_core::mirror::{Announce, Mirrors, Remotes};
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
    fn time_running_counts_from_the_start() {
        let app = mock_app();
        assert!(app.manage(Started::now()));
        std::thread::sleep(Duration::from_millis(20));
        let first = time_running(app.state());
        let second = time_running(app.state());
        assert!(first >= 20.0, "{first}");
        assert!(second >= first, "{first} then {second}");
    }

    #[test]
    fn a_measurement_answers_with_the_line_it_wrote() {
        let measurement: Measurement = serde_json::from_str(
            r#"{"seconds": 2, "frames": null, "startup_ms": null, "keys": null, "view_queries": null,
                "refreshes": null, "issues": null, "long_frames": [{"at_s": 1, "ms": 30, "cause": null}]}"#,
        )
        .unwrap();
        let line = record_measurement(measurement.clone());
        assert_eq!(line, measurement.summary());
    }

    #[test]
    fn a_miss_answers_with_the_line_it_wrote() {
        let miss: Miss = serde_json::from_str(r#"{"kind": "view_query", "ms": 73}"#).unwrap();
        assert_eq!(record_miss(miss.clone()), miss.line());
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
        assert!(app.manage(SettingsOwner::start(Ok(SettingsFiles::load(
            folders.clone()
        )))));
        assert_eq!(home(app.state()), Home::default());

        let session = ended_session();
        let mirrors = mirrors_cloning_nothing(Ok(folders.clone()));
        let work = RepositoryWork {
            session: session.requests(),
            mirrors: mirrors.requests(),
        };
        let added = add_named_repository(
            "https://github.com/python/cpython",
            RepositoryAccess::Other,
            &app.state(),
            &work,
        );
        let after = home(app.state());
        mirrors.stop(Duration::from_secs(10));
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(added.unwrap(), cpython());
        assert_eq!(after, cpython());
    }

    #[test]
    fn a_name_that_isnt_one_is_refused_before_anything_else_happens() {
        let owner = SettingsOwner::start(Err(no_folders()));
        let session = ended_session();
        let mirrors = mirrors_cloning_nothing(Err(no_folders()));
        let work = RepositoryWork {
            session: session.requests(),
            mirrors: mirrors.requests(),
        };

        let adding =
            add_named_repository("nonsense", RepositoryAccess::Other, &owner, &work).unwrap_err();
        let checking = check_typed_repository("nonsense", &session.requests()).unwrap_err();
        let reading = read_workbench("nonsense", &WorkbenchView::Whole, &owner, &session.reader())
            .unwrap_err();

        assert_eq!(adding.kind, ErrorKind::NotFound);
        assert_eq!(checking.kind, ErrorKind::NotFound);
        assert_eq!(reading.kind, ErrorKind::NotFound);
    }

    /// Mirrors whose clones fail at once, from a folder of remotes that isn't there.
    fn mirrors_cloning_nothing(folders: Result<Folders, Error>) -> Mirrors {
        Mirrors::start(
            folders,
            Remotes::under("no-remotes/".to_owned()),
            Announce::cloning_only(Box::new(|_| {})),
        )
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
