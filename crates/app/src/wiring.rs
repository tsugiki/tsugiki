//! The window's commands and events, set up on any Tauri runtime. `startup.rs` calls this on the real one.
//! The tests here call it on Tauri's mock one, so they check this wiring without a window.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tsugiki_core::definitions::{self, Definitions, ServerName};
use tsugiki_core::diff::{Diffs, Highlighter};
use tsugiki_core::error::Error;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::latest::{self, LatestWriter};
use tsugiki_core::mirror::{Announce, MirrorRequests, Mirrors, Remotes};
use tsugiki_core::releases::{AskPublic, ReleaseCheck};
use tsugiki_core::search::TextSearch;
use tsugiki_core::session::{Connections, Intervals, Session};
use tsugiki_core::settings::{SettingsFiles, SettingsOwner};
use tsugiki_core::status::{Change, Status, StatusBoard};

use crate::first_frame;
use crate::{
    Changed, CloningChanged, DiffWork, FailedHeads, HeadsFetchFailed, HeadsFetched,
    HourPointsChanged, LanguageServersChanged, LastSyncChanged, MirrorProblemChanged,
    MirrorQuestionChanged, MirrorStatesChanged, MirrorsHeld, MirrorsHeldChanged, NewVersionFound,
    PickPath, Picker, ReadFailed, RepositoryWork, StatusChanged, SyncActivityChanged,
    TextSearchChanged,
};

/// How often the open Repository refreshes: a refresh costs a few points, and this is about 60 points an
/// hour, out of 5,000 (ADR 0015).
const REPOSITORY_REFRESH: Duration = Duration::from_secs(180);

/// The time between two checks of GitHub Releases for a newer version, after the one at start-up. GitHub
/// lets one address ask 60 times an hour without a token, and this asks four times a day.
const RELEASE_CHECK: Duration = Duration::from_secs(6 * 60 * 60);

/// The world outside the window that Tsugiki reaches: GitHub's API and `gh`, GitHub Releases without a
/// token, where mirrors are cloned from, and the system's picker of folders and programs.
pub(crate) struct Outside {
    pub(crate) github: GitHubAccess,
    pub(crate) releases: AskPublic,
    pub(crate) remotes: Remotes,
    pub(crate) pick: PickPath,
}

/// The threads that work for the window: the session, the mirrors' one writer, and the check for a new
/// version unless the settings turn it off. The check ends when this is dropped: it writes nothing, so
/// quitting needn't wait for it.
pub(crate) struct Core {
    pub(crate) session: Session,
    mirrors: Mirrors,
    definitions: Definitions,
    _releases: Option<ReleaseCheck>,
}

impl Core {
    /// Stops the session, the mirrors and the language servers, and gives them `patience` between them to
    /// finish.
    pub(crate) fn stop(&self, patience: Duration) {
        let deadline = Instant::now() + patience;
        self.mirrors.begin_stopping();
        self.session.stop(patience);
        self.mirrors
            .wait_for_end(deadline.saturating_duration_since(Instant::now()));
        self.definitions
            .stop(deadline.saturating_duration_since(Instant::now()));
    }
}

/// A piece of state the window both asks for and hears of: a command reads its newest value, and an event
/// says it changed.
struct WindowState<T, R: Runtime> {
    latest: LatestWriter<T>,
    app: AppHandle<R>,
}

impl<T: Clone + Send + 'static, R: Runtime> WindowState<T, R> {
    /// State that starts as `first`, whose reader `app` manages for the command. `what` names it in the
    /// failure when something else already manages a reader of its type.
    fn manage(app: &AppHandle<R>, first: T, what: &str) -> Self {
        let (latest, reader) = latest::channel(first);
        assert!(app.manage(reader), "only this manages {what}");
        Self {
            latest,
            app: app.clone(),
        }
    }

    /// Sets the state to `value`, then sends the event `event` makes of it. A window that starts listening
    /// too late for the event asks the command once it has loaded, so the value has to be there first.
    fn set<E: Event + Serialize + Clone>(&mut self, value: T, event: impl FnOnce(T) -> E) {
        self.latest.set(&value);
        log_if_unsent(event(value).emit(&self.app));
    }
}

/// The board the session announces app-wide status on. It keeps the newest of each for its command, then
/// sends the event, and tells the mirrors of each status, which they follow.
fn status_board<R: Runtime>(app: &AppHandle<R>, following: MirrorRequests) -> StatusBoard {
    let mut status_state = WindowState::manage(app, Status::default(), "the status");
    let mut sync_state = WindowState::manage(app, None, "the last sync");
    let mut activity_state = WindowState::manage(app, None, "the sync activity");
    let mut points_state = WindowState::manage(app, None, "the hour's points");
    let changes_handle = app.clone();
    let failed_reads_handle = app.clone();
    StatusBoard::new(Box::new(move |status| {
        following.follow(status);
        status_state.set(status.clone(), StatusChanged);
    }))
    .with_changes(Box::new(move |change| {
        log_if_unsent(Changed(change.clone()).emit(&changes_handle));
    }))
    // Only a view that is listening asks for a read, so a window that starts late has missed nothing.
    .with_failed_reads(Box::new(move |failed| {
        log_if_unsent(ReadFailed(failed.clone()).emit(&failed_reads_handle));
    }))
    .with_last_syncs(Box::new(move |last_sync| {
        sync_state.set(Some(last_sync.clone()), |_| {
            LastSyncChanged(last_sync.clone())
        });
    }))
    .with_sync_activity(Box::new(move |activity| {
        activity_state.set(activity.cloned(), SyncActivityChanged);
    }))
    .with_hour_points(Box::new(move |points| {
        points_state.set(points.copied(), HourPointsChanged);
    }))
}

/// Reads the settings, starts the session and the mirrors, and gives the commands what they ask. Events need
/// `mount_events` to have run on `app`.
pub(crate) fn connect_core<R: Runtime>(
    app: &AppHandle<R>,
    folders: Result<Folders, Error>,
    outside: Outside,
    refresh: Duration,
) -> Core {
    assert!(
        app.manage(Picker(outside.pick)),
        "only this manages the picker"
    );
    // Two small files, read before the page loads so `settings` and `home` can always answer. A
    // `settings_path` on a slow drive would delay the page. Without folders the defaults apply, and the
    // session shows why. A thread of their own owns the files from here, so saving doesn't wait on GitHub.
    let settings = SettingsOwner::start(folders.clone().map(SettingsFiles::load));
    turn_to_start_theme(app, &settings);
    // Settings Tsugiki can't read fall back to the defaults, which check, though the file may turn the check
    // off. So only settings read without a problem start it.
    let check_releases_on = {
        let loaded = &settings.view().loaded;
        folders.is_ok() && loaded.problems.is_empty() && loaded.settings.releases.check
    };
    let announce = mirror_announcements(app);
    let mirrors = Mirrors::start(folders.clone(), outside.remotes, announce);
    let board = status_board(app, mirrors.requests());
    let intervals = Intervals {
        status: refresh,
        repository: REPOSITORY_REFRESH,
    };
    let diff_folders = folders.clone();
    // The session hands the mirrors the rules' wants, and takes the patch-ids they read.
    let connections = Connections {
        github: outside.github,
        mirrors: mirrors.requests(),
    };
    let session = Session::start(board, folders, connections, intervals);
    let fetching = session.fetching();
    let (checkouts, view, idle_view) = (mirrors.requests(), settings.reader(), settings.reader());
    let rows_handle = app.clone();
    let definitions = Definitions::start(definitions::Sources {
        checkout: Box::new(move |repository| checkouts.lookup_checkout(repository.clone())),
        servers: Box::new(move |server| {
            let servers = view.get().language_servers;
            match server {
                ServerName::Ty => servers.ty,
                ServerName::RustAnalyzer => servers.rust_analyzer,
                ServerName::Clangd => servers.clangd,
            }
        }),
        idle: Box::new(move || idle_view.get().language_servers.idle),
        // A window that starts listening too late asks `language_servers`, which reads what the thread
        // wrote before this.
        changed: Box::new(move |rows| {
            log_if_unsent(LanguageServersChanged(rows.to_vec()).emit(&rows_handle));
        }),
    });
    let diffs = DiffWork {
        folders: diff_folders,
        reader: session.reader(),
        diffs: Diffs::default(),
        fetching: fetching.clone(),
        highlighter: Highlighter::start(),
        definitions: definitions.clone(),
    };
    assert!(app.manage(diffs), "only this manages the diffs");
    let search_handle = app.clone();
    // A window that starts listening too late asks `text_search_state`, which reads what the thread wrote
    // before this.
    let search = TextSearch::start(
        mirrors.requests(),
        Box::new(move || log_if_unsent(TextSearchChanged.emit(&search_handle))),
    );
    assert!(app.manage(search), "only this manages the text search");
    let work = RepositoryWork {
        fetching: fetching.clone(),
    };
    let view = settings.view();
    work.start(&view.home, &view.mirror_sources);
    // From here the owner hands the session each Repository's settings it saves otherwise, before the window
    // can ask for a change.
    let session_settings = fetching.clone();
    settings.tell_session(Box::new(move |repository, by| {
        session_settings.settings_changed(repository, by);
    }));
    // The Repositories added before 0.2 have no mirror yet, and get theirs after the open one's. One the user
    // hasn't said where its objects come from asks when its turn comes.
    for repository in &view.loaded.settings.repositories {
        let source = view.mirror_sources.get(&repository.name).cloned();
        fetching.clone_mirror(repository.name.clone(), source);
    }
    assert!(app.manage(settings), "only this manages the settings");
    assert!(app.manage(fetching), "only this manages the requests");
    assert!(
        app.manage(work),
        "only this manages the work on Repositories"
    );
    let reader = session.reader();
    assert!(app.manage(reader), "only this manages the reader");
    let releases = check_releases(app, check_releases_on.then_some(outside.releases));
    Core {
        session,
        mirrors,
        definitions,
        _releases: releases,
    }
}

/// Turns each window to the side the settings keep, when Tsugiki can tell it without the `themes` folder: a
/// built-in Theme's, or a mode's from a file written before (#755). `startup.rs` opened the window on the
/// system's side, and this turns it before the page loads, so its title bar doesn't flip once the page has
/// read the folder. The page asks `turn_window` to turn it for a Theme from the folder.
fn turn_to_start_theme<R: Runtime>(app: &AppHandle<R>, settings: &SettingsOwner) {
    let Some(side) = settings
        .view()
        .loaded
        .settings
        .appearance
        .theme
        .known_side()
    else {
        return;
    };
    for window in app.webview_windows().into_values() {
        first_frame::turn(Some(side), &|theme| window.set_theme(theme));
    }
}

/// Starts the check for a new version with `ask`. Without it Tsugiki sends Releases nothing, and the command
/// answers that no version is out. Each version the check finds is set for the command that asks before it
/// goes out as an event, as [`WindowState`] sets each.
fn check_releases<R: Runtime>(app: &AppHandle<R>, ask: Option<AskPublic>) -> Option<ReleaseCheck> {
    let mut version_state = WindowState::manage(app, None, "the new version");
    ask.map(|ask| {
        ReleaseCheck::start(
            ask,
            RELEASE_CHECK,
            Box::new(move |new| {
                version_state.set(Some(new.clone()), |_| NewVersionFound(new.clone()));
            }),
        )
    })
}

/// Tells the window what the mirrors' one writer says. Each change is set for the commands that ask before it
/// goes out as an event, as [`WindowState`] sets each.
fn mirror_announcements<R: Runtime>(app: &AppHandle<R>) -> Announce {
    let mut cloning_state = WindowState::manage(app, None, "the clone");
    let mut problem_state = WindowState::manage(app, None, "the mirror's problem");
    let mut question_state = WindowState::manage(app, None, "the mirror's question");
    let mut states_state = WindowState::manage(app, Vec::new(), "the mirrors' states");
    let held_what = "whether the mirrors' worker holds back git";
    let mut held_state = WindowState::manage(app, MirrorsHeld(false), held_what);
    let heads_handle = app.clone();
    let failed_handle = app.clone();
    Announce {
        cloning: Box::new(move |cloning| cloning_state.set(cloning.cloned(), CloningChanged)),
        fetched: Box::new(move |repository, numbers| {
            let change = Change {
                repository: repository.clone(),
                items: numbers.to_vec(),
            };
            log_if_unsent(HeadsFetched(change).emit(&heads_handle));
        }),
        failed: Box::new(move |repository, numbers, why| {
            let failed = FailedHeads {
                heads: Change {
                    repository: repository.clone(),
                    items: numbers.to_vec(),
                },
                why: why.clone(),
            };
            log_if_unsent(HeadsFetchFailed(failed).emit(&failed_handle));
        }),
        problem: Box::new(move |problem| {
            problem_state.set(problem.cloned(), MirrorProblemChanged);
        }),
        question: Box::new(move |repository| {
            question_state.set(repository.cloned(), MirrorQuestionChanged);
        }),
        states: Box::new(move |states| states_state.set(states.to_vec(), MirrorStatesChanged)),
        held: Box::new(move |held| {
            held_state.set(MirrorsHeld(held), |_| MirrorsHeldChanged(held));
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
mod tests;
