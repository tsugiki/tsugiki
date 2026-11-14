//! The window's commands and events, set up on any Tauri runtime. `startup.rs` calls this on the real one.
//! The tests here call it on Tauri's mock one, so they check this wiring without a window.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tsugiki_core::definitions::{self, Definitions, ServerName};
use tsugiki_core::diff::{Diffs, Highlighter};
use tsugiki_core::error::Error;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::latest;
use tsugiki_core::mirror::{Announce, Mirrors, Remotes};
use tsugiki_core::releases::{AskPublic, ReleaseCheck};
use tsugiki_core::session::{Intervals, Session};
use tsugiki_core::settings::{SettingsFiles, SettingsOwner};
use tsugiki_core::status::{Change, Status, StatusBoard};

use crate::first_frame;
use crate::{
    Changed, CloningChanged, DiffWork, FailedHeads, FolderPicker, HeadsFetchFailed, HeadsFetched,
    LastSyncChanged, MirrorProblemChanged, MirrorQuestionChanged, MirrorStatesChanged, MirrorsHeld,
    MirrorsHeldChanged, NewVersionFound, PickFolder, ReadFailed, RepositoryWork, StartMode,
    StatusChanged, SyncActivityChanged,
};

/// How often the open Repository refreshes: a refresh costs a few points, and this is about 60 points an
/// hour, out of 5,000 (ADR 0015).
const REPOSITORY_REFRESH: Duration = Duration::from_secs(180);

/// The time between two checks of GitHub Releases for a newer version, after the one at start-up. GitHub
/// lets one address ask 60 times an hour without a token, and this asks four times a day.
const RELEASE_CHECK: Duration = Duration::from_secs(6 * 60 * 60);

/// The world outside the window that Tsugiki reaches: GitHub's API and `gh`, GitHub Releases without a
/// token, where mirrors are cloned from, and the system's folder picker.
pub(crate) struct Outside {
    pub(crate) github: GitHubAccess,
    pub(crate) releases: AskPublic,
    pub(crate) remotes: Remotes,
    pub(crate) pick_folder: PickFolder,
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
    // Two small files, read before the page loads so `settings` and `home` can always answer. A
    // `settings_path` on a slow drive would delay the page. Without folders the defaults apply, and the
    // session shows why. A thread of their own owns the files from here, so saving doesn't wait on GitHub.
    let settings = SettingsOwner::start(folders.clone().map(SettingsFiles::load));
    turn_to_start_appearance(app, &settings);
    // Settings Tsugiki can't read fall back to the defaults, which check, though the file may turn the check
    // off. So only settings read without a problem start it.
    let check_releases_on = {
        let loaded = &settings.view().loaded;
        folders.is_ok() && loaded.problems.is_empty() && loaded.settings.releases.check
    };
    let status_handle = app.clone();
    let changes_handle = app.clone();
    let failed_reads_handle = app.clone();
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
    // Only a view that is listening asks for a read, so a window that starts late has missed nothing.
    .with_failed_reads(Box::new(move |failed| {
        log_if_unsent(ReadFailed(failed.clone()).emit(&failed_reads_handle));
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
    .with_heads(Box::new(move |repository, work| {
        fetching.fetch_heads(repository.clone(), work.heads);
        fetching.keep_reviewed(repository.clone(), work.reviewed);
        fetching.read_patch_ids(repository.clone(), work.patches);
    }));
    let intervals = Intervals {
        status: refresh,
        repository: REPOSITORY_REFRESH,
    };
    let diff_folders = folders.clone();
    let session = Session::start(board, folders, outside.github, intervals);
    let (checkouts, view) = (mirrors.requests(), settings.reader());
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
    });
    let diffs = DiffWork {
        folders: diff_folders,
        reader: session.reader(),
        diffs: Diffs::default(),
        mirrors: mirrors.requests(),
        highlighter: Highlighter::start(),
        definitions: definitions.clone(),
    };
    assert!(app.manage(diffs), "only this manages the diffs");
    let requests = session.requests();
    // The mirrors start first, as the session's board hands them heads, so they learn here where the
    // patch-ids they read go.
    mirrors
        .requests()
        .hand_patches_to(requests.clone().patch_ids_keeper());
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
    let releases = check_releases(app, check_releases_on.then_some(outside.releases));
    Core {
        session,
        mirrors,
        definitions,
        _releases: releases,
    }
}

/// Turns each window to the mode `settings` holds, and keeps the mode for `themes` (#517). `startup.rs`
/// opened the window first, on the system's side, so a Light or Dark mode turns it now, before the page asks
/// for its themes. Both keep to the mode the settings held at start, as the docs say, so a later read of the
/// files can't turn the page without the title bar.
fn turn_to_start_appearance<R: Runtime>(app: &AppHandle<R>, settings: &SettingsOwner) {
    let mode = settings.view().loaded.settings.appearance.theme;
    for window in app.webview_windows().into_values() {
        first_frame::turn(mode, &|theme| window.set_theme(theme));
    }
    assert!(app.manage(StartMode(mode)), "only this manages the mode");
}

/// Starts the check for a new version with `ask`. Without it Tsugiki sends Releases nothing, and the command
/// answers that no version is out. Each version the check finds is set for the command that asks before it
/// goes out as an event, for the reason `connect_core` gives for the status.
fn check_releases<R: Runtime>(app: &AppHandle<R>, ask: Option<AskPublic>) -> Option<ReleaseCheck> {
    let (mut latest_version, version_reader) = latest::channel(None);
    assert!(
        app.manage(version_reader),
        "only this manages the new version"
    );
    let handle = app.clone();
    ask.map(|ask| {
        ReleaseCheck::start(
            ask,
            RELEASE_CHECK,
            Box::new(move |new| {
                latest_version.set(&Some(new.clone()));
                log_if_unsent(NewVersionFound(new.clone()).emit(&handle));
            }),
        )
    })
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
    let (mut latest_states, states_reader) = latest::channel(Vec::new());
    assert!(
        app.manage(states_reader),
        "only this manages the mirrors' states"
    );
    let (mut latest_held, held_reader) = latest::channel(MirrorsHeld(false));
    assert!(
        app.manage(held_reader),
        "only this manages whether the mirrors' worker holds back git"
    );
    let cloning_handle = app.clone();
    let states_handle = app.clone();
    let problem_handle = app.clone();
    let question_handle = app.clone();
    let heads_handle = app.clone();
    let failed_handle = app.clone();
    let held_handle = app.clone();
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
            let problem = problem.cloned();
            latest_problem.set(&problem);
            log_if_unsent(MirrorProblemChanged(problem).emit(&problem_handle));
        }),
        question: Box::new(move |repository| {
            let repository = repository.cloned();
            latest_question.set(&repository);
            log_if_unsent(MirrorQuestionChanged(repository).emit(&question_handle));
        }),
        states: Box::new(move |states| {
            let states = states.to_vec();
            latest_states.set(&states);
            log_if_unsent(MirrorStatesChanged(states).emit(&states_handle));
        }),
        held: Box::new(move |held| {
            latest_held.set(&MirrorsHeld(held));
            log_if_unsent(MirrorsHeldChanged(held).emit(&held_handle));
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
