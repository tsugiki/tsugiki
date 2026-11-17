//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

#![forbid(unsafe_code)]

mod first_frame;
mod navigation;
mod slow_commands;
mod startup;
mod wiring;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::build::Build;
use tsugiki_core::definitions::{
    Answer, Code, Definitions, DiffEnd, HitFile, HitLines, Lookup, OnIndex, OnWaiting, ServerName,
    ServerRow, Ticket, server_log,
};
use tsugiki_core::diff::{DiffOf, DiffRow, Diffs, Found, HeadLine, Highlighter, PrAt};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::facts::CommitSha;
use tsugiki_core::fetching::Fetching;
use tsugiki_core::filters::{FilterQuery, OnWorkbench};
use tsugiki_core::folders::Folders;
use tsugiki_core::github::points::HourPoints;
use tsugiki_core::highlight::Span;
use tsugiki_core::latest::LatestReader;
use tsugiki_core::measurement::Measurement;
use tsugiki_core::mirror::{
    CloneRefusal, Cloning, LookupAt, MirrorProblem, RepositoryMirror, Unfetched,
};
use tsugiki_core::mirror_source::{MirrorSource, MirrorSources};
use tsugiki_core::misses::Miss;
use tsugiki_core::releases::NewVersion;
use tsugiki_core::repository::RepositoryName;
use tsugiki_core::repository_settings::{MirrorCost, NameCheck, RepositorySettings};
use tsugiki_core::search::{Query, SearchId, SearchState, TextSearch};
use tsugiki_core::settings::{
    CommandId, Home, LoadedSettings, PaneLayout, SettingsOwner, SettingsView, TokenSource,
};
use tsugiki_core::status::{Change, FailedRead, LastSync, Status, SyncActivity};
use tsugiki_core::store::StoreReader;
use tsugiki_core::theme::ThemeKind;
use tsugiki_core::version::Version;
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::work::SeenMark;
use tsugiki_core::workbench::inbox::{Inbox, InboxPage};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_files::{DiffSources, FileLine, FilesRead, LineInDiff, PrFiles};
use tsugiki_core::workbench::pr_view::PrView;
use tsugiki_core::workbench::quick_open;
use tsugiki_core::workbench::{Workbench, WorkbenchView};

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about(&version(), Build::CURRENT)
}

/// Opens WebView2's DevTools for the window that asks, from the palette (#460). Only a development build
/// has DevTools: a release turns the browser's keys off and refuses here too (ADR 0013).
#[tauri::command]
#[specta::specta]
fn open_devtools<R: tauri::Runtime>(window: tauri::WebviewWindow<R>) -> Result<(), Error> {
    open_devtools_in(Build::CURRENT, &window)
}

fn open_devtools_in<R: tauri::Runtime>(
    build: Build,
    window: &tauri::WebviewWindow<R>,
) -> Result<(), Error> {
    match build {
        Build::Release => Err(Error::new(
            ErrorKind::Internal,
            "A release build of Tsugiki has no DevTools.",
        )),
        Build::Development => {
            show_devtools(window);
            Ok(())
        }
    }
}

/// Tauri has DevTools only in a debug build, and a release never asks for them.
#[cfg(debug_assertions)]
fn show_devtools<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    window.open_devtools();
}

#[cfg(not(debug_assertions))]
fn show_devtools<R: tauri::Runtime>(_: &tauri::WebviewWindow<R>) {}

/// This build's version, with the commit `build.rs` found, if it found one.
pub(crate) fn version() -> Version {
    Version::new(option_env!("TSUGIKI_COMMIT"))
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

type LatestHourPoints = LatestReader<Option<HourPoints>>;

/// The hour's GitHub points, for a window that has just loaded. Nothing, until GitHub's first answer of the
/// run says, and from the hour's reset until an answer of the next hour says.
#[tauri::command]
#[specta::specta]
fn hour_points(latest: State<'_, LatestHourPoints>) -> Option<HourPoints> {
    latest.get()
}

type LatestCloning = LatestReader<Option<Cloning>>;

/// The clone of a mirror under way, for a window that has just loaded.
#[tauri::command]
#[specta::specta]
fn cloning(latest: State<'_, LatestCloning>) -> Option<Cloning> {
    latest.get()
}

type LatestMirrorProblem = LatestReader<Option<MirrorProblem>>;

/// The latest reason no mirror can be made, for a window as it loads.
#[tauri::command]
#[specta::specta]
fn mirror_problem(latest: State<'_, LatestMirrorProblem>) -> Option<MirrorProblem> {
    latest.get()
}

type LatestMirrorQuestion = LatestReader<Option<RepositoryName>>;

/// The first Repository whose mirror waits to hear where it gets its objects, for a window as it loads.
#[tauri::command]
#[specta::specta]
fn mirror_question(latest: State<'_, LatestMirrorQuestion>) -> Option<RepositoryName> {
    latest.get()
}

type LatestNewVersion = LatestReader<Option<NewVersion>>;

/// The newest version out that is newer than the running one, for a window as it loads. Nothing, until a
/// check finds one.
#[tauri::command]
#[specta::specta]
fn new_version(latest: State<'_, LatestNewVersion>) -> Option<NewVersion> {
    latest.get()
}

type LatestMirrorStates = LatestReader<Vec<RepositoryMirror>>;

/// The state of each Repository's mirror the worker has heard of, for a window as it loads.
#[tauri::command]
#[specta::specta]
fn mirror_states(latest: State<'_, LatestMirrorStates>) -> Vec<RepositoryMirror> {
    latest.get()
}

/// Whether the mirrors' worker holds back git while GitHub can't be reached. A type of its own, as Tauri
/// tells managed state apart by type, and another `bool` would take its place.
#[derive(Clone, Copy)]
struct MirrorsHeld(bool);

type LatestMirrorsHeld = LatestReader<MirrorsHeld>;

/// Whether the mirrors' worker holds back git while GitHub can't be reached, for a window as it loads.
#[tauri::command]
#[specta::specta]
fn mirrors_held(latest: State<'_, LatestMirrorsHeld>) -> bool {
    latest.get().0
}

/// Says why the clone in `folder` can't lend its objects to `repository`'s mirror, or nothing if it can.
fn check_named_clone(
    repository: &str,
    folder: &Path,
    work: &RepositoryWork,
) -> Result<Option<CloneRefusal>, Error> {
    let repository = RepositoryName::parse(repository)?;
    Ok(work.fetching.check_clone(folder, &repository).err())
}

/// Starts `repository`'s clone ahead of the others, with its objects from `source`, and saves the answer in
/// `machine.toml`.
fn choose_named_source(
    repository: &str,
    source: MirrorSource,
    owner: &SettingsOwner,
    work: &RepositoryWork,
) -> Result<(), Error> {
    let repository = RepositoryName::parse(repository)?;
    // The clone goes ahead even if `machine.toml` can't keep the answer. The next launch asks again.
    work.fetching
        .clone_mirror_next(repository.clone(), Some(source.clone()));
    drop(owner.save_mirror_source(repository, source)?);
    Ok(())
}

/// Opens the system's picker for a folder or a program, and gives the one the user picked, if they picked
/// one.
pub(crate) type PickPath = Arc<dyn Fn(Pick) -> Option<PathBuf> + Send + Sync>;

/// What the system's picker picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pick {
    Folder,
    /// A language server's program, for its row in Settings › Advanced.
    Program,
}

/// The system's picker, which the commands share.
pub(crate) struct Picker(PickPath);

/// The settings in force once `settings` was read or applied. The language servers' thread hears they may
/// have changed either way, as a failed read leaves the last settings in force.
fn settings_in_force(
    settings: Result<SettingsView, Error>,
    work: &DiffWork,
) -> Result<LoadedSettings, Error> {
    work.definitions.reconfigured();
    settings.map(|view| view.loaded)
}

/// The log that language server `server` writes for `repository`, which lives beside its Lookup checkout,
/// once the server has written one.
fn server_log_of(repository: &str, server: ServerName, work: &DiffWork) -> Result<PathBuf, Error> {
    let name = RepositoryName::parse(repository)?;
    let folders = work.folders.as_ref().map_err(Clone::clone)?;
    let log = server_log(&folders.lookup(&name), server);
    if !log.is_file() {
        let message = format!("{} has written no log for {name} yet.", server.as_str());
        return Err(Error::new(ErrorKind::NotFound, &message));
    }
    Ok(log)
}

/// Looks at git and the disk again, and tries the clones that waited for them, and returns at once.
#[tauri::command]
#[specta::specta]
fn check_mirrors_again(work: State<'_, RepositoryWork>) {
    work.fetching.check_mirrors_again();
}

/// Refreshes the open Repository now, and returns at once.
#[tauri::command]
#[specta::specta]
fn refresh(requests: State<'_, Fetching>) {
    requests.refresh();
}

/// Tells the core the user opened an Issue, which reads what it says from GitHub, and returns at once. Its
/// Last seen stays: only Mark Seen moves it.
#[tauri::command]
#[specta::specta]
fn open_issue(repository: &str, number: u32, requests: State<'_, Fetching>) -> Result<(), Error> {
    requests.fetch_issue(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Tells the core the user opened PR `number`, which reads its Conversation from GitHub and asks the mirror
/// first for its head if the mirror lacks it. The Files tab reads the diff once it shows. Its Last seen
/// stays.
fn open_pull_request(repository: &str, number: u32, work: &DiffWork) -> Result<(), Error> {
    let name = RepositoryName::parse(repository)?;
    work.fetching.fetch_pull_request(name.clone(), number);
    if let Some(head) = PrFiles::head_in_mirror(&work.sources()?, &name, number)? {
        work.fetching.pull_request_opened(name, number, head);
    }
    Ok(())
}

/// Mark Seen on what `mark` names, as the window drew it: an item, and for an Issue every PR in its tree.
/// Returns at once, and the `changed` event follows.
#[tauri::command]
#[specta::specta]
fn mark_seen(repository: &str, mark: SeenMark, requests: State<'_, Fetching>) -> Result<(), Error> {
    requests.mark_seen(RepositoryName::parse(repository)?, mark);
    Ok(())
}

/// Mark Unseen on item `number`, and returns at once.
#[tauri::command]
#[specta::specta]
fn mark_unseen(repository: &str, number: u32, requests: State<'_, Fetching>) -> Result<(), Error> {
    requests.mark_unseen(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Asks the core to read a PR's Conversation on screen now, and returns at once (ADR 0015).
#[tauri::command]
#[specta::specta]
fn fetch_pull_request(
    repository: &str,
    number: u32,
    requests: State<'_, Fetching>,
) -> Result<(), Error> {
    requests.fetch_pull_request(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// Asks the core to read what an Issue on screen says now, and returns at once (ADR 0015).
#[tauri::command]
#[specta::specta]
fn fetch_issue(repository: &str, number: u32, requests: State<'_, Fetching>) -> Result<(), Error> {
    requests.fetch_issue(RepositoryName::parse(repository)?, number);
    Ok(())
}

/// The settings Tsugiki last read, and what was wrong with their files (ADR 0014).
#[tauri::command]
#[specta::specta]
fn settings(owner: State<'_, SettingsOwner>) -> LoadedSettings {
    owner.view().loaded
}

/// Turns the window that asks to `side`, its title bar and native controls with it, or back to following
/// the system with none (#755). The page asks for the side of the theme it is about to show, once it has the
/// `themes` answer, and sends the next ask only after this one answers. Back to the system, it says the side
/// the window is on, or none if the window can't say.
///
/// It doesn't wait, so Tauri runs it on the main thread, where the window's theme is read at once.
#[tauri::command]
#[specta::specta]
fn turn_window<R: tauri::Runtime>(
    side: Option<ThemeKind>,
    window: tauri::WebviewWindow<R>,
) -> Option<ThemeKind> {
    let themed = first_frame::ThemedWindow {
        set_theme: &|theme| window.set_theme(theme),
        theme: &|| window.theme(),
    };
    first_frame::turn_window(side, &themed)
}

/// What the window opens on.
#[tauri::command]
#[specta::specta]
fn home(owner: State<'_, SettingsOwner>) -> Home {
    owner.view().home
}

/// Where the token comes from, for the Settings dialog's Account page (#429).
#[tauri::command]
#[specta::specta]
fn token_source() -> TokenSource {
    TokenSource::IN_USE
}

/// How the user laid out the panes on this machine: the Sidebar's and the Workbench list's widths (#187), and
/// whether the Sidebar (#234) and the list (#242) show.
#[tauri::command]
#[specta::specta]
fn pane_layout(owner: State<'_, SettingsOwner>) -> PaneLayout {
    owner.view().panes
}

/// The command palette's commands the user ran on this machine, most recently run first, which the palette
/// lists first (#556).
#[tauri::command]
#[specta::specta]
fn recent_commands(owner: State<'_, SettingsOwner>) -> Vec<CommandId> {
    owner.recent_commands()
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
    let summary = measurement.summary(&version());
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

/// Says the Repository the user typed is one Tsugiki has, or else asks GitHub what adding it would load.
fn check_typed_repository(
    input: &str,
    owner: &SettingsOwner,
    requests: &Fetching,
) -> Result<NameCheck, Error> {
    owner.check_name(RepositoryName::parse(input)?, &mut |name| {
        requests.check(name)
    })
}

/// Asks GitHub what the mirror of `repository` takes, with the check Add repository makes. The Files tab asks
/// for a Repository Tsugiki has.
fn check_mirror_cost(repository: &str, requests: &Fetching) -> Result<MirrorCost, Error> {
    Ok(requests.check(RepositoryName::parse(repository)?)?.mirror)
}

/// The user's choice in Add repository: the Repository, by the name GitHub gave it, and where its mirror gets
/// its objects. The name stays text until the core parses it, so a name that isn't one gets the same refusal
/// as everywhere else.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct NewRepository {
    name: String,
    source: MirrorSource,
}

/// Adds the Repository, and starts its first sync (ADR 0015) and its mirror's clone.
fn add_named_repository(
    repository: NewRepository,
    owner: &SettingsOwner,
    work: &RepositoryWork,
) -> Result<Home, Error> {
    let NewRepository { name, source } = repository;
    let view = owner.add_repository(RepositoryName::parse(&name)?, source)?;
    work.start(&view.home, &view.mirror_sources);
    Ok(view.home)
}

/// The switch in a Filter's view: the Repository, the Filter's search, and whether its items go on the
/// Workbench (#819). The name stays text until the core parses it, as in `NewRepository`.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct FilterSwitch {
    repository: String,
    query: FilterQuery,
    on: OnWorkbench,
}

/// Puts the Filter that `switch` names on the Workbench, or takes it off. The owner hands the session the
/// settings saved, which work out Reach with them at once. The open Repository stays as it is.
fn switch_filter(switch: FilterSwitch, owner: &SettingsOwner) -> Result<(), Error> {
    let FilterSwitch {
        repository,
        query,
        on,
    } = switch;
    let name = RepositoryName::parse(&repository)?;
    owner.put_on_workbench(name, query, on).map(drop)
}

/// Makes the Repository the user picked the open one, and hands it to the session and the mirrors.
fn open_named_repository(
    name: &str,
    owner: &SettingsOwner,
    work: &RepositoryWork,
) -> Result<Home, Error> {
    let view = owner.open_repository(RepositoryName::parse(name)?)?;
    work.switch(&view.home);
    Ok(view.home)
}

/// The threads that work on Repositories, the session's sync queue and the mirrors' one writer, which the
/// commands ask through `fetching`.
struct RepositoryWork {
    fetching: Fetching,
}

impl RepositoryWork {
    /// Opens the Repository the window launches on or the user added. `sources` says where its mirror's
    /// objects come from, if the user has said.
    fn start(&self, home: &Home, sources: &MirrorSources) {
        if let Home::Repository { repository, .. } = home {
            self.open(repository, sources.get(&repository.name).cloned());
        }
    }

    /// Opens the Repository the user switched to. It hands the mirrors no answer: they have each one saved
    /// at launch, and each the user gave since, which `machine.toml` may have failed to keep.
    fn switch(&self, home: &Home) {
        if let Home::Repository { repository, .. } = home {
            self.open(repository, None);
        }
    }

    /// Syncs `repository`, under the name the window knows it by, so the window recognises the changes. It
    /// takes every other sync step and the timed refresh from the one open before (ADR 0015), whose sync, if
    /// unfinished, goes on in its turns. Its mirror's clone goes ahead of the others waiting, after the one
    /// under way, and doesn't wait on the sync. While neither `source` nor the mirrors hold an answer of
    /// where its objects come from, the clone waits for one.
    fn open(&self, repository: &RepositorySettings, source: Option<MirrorSource>) {
        self.fetching.sync(repository.clone());
        self.fetching
            .clone_mirror_next(repository.name.clone(), source);
    }
}

fn read_workbench(
    repository: &str,
    view: &WorkbenchView,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Workbench, Error> {
    let repository = repository_settings(repository, owner)?;
    Workbench::read(reader, &repository, view, owner.view().panes.order)
}

fn read_inbox(
    repository: &str,
    page: InboxPage,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Inbox, Error> {
    Inbox::read_now(reader, &repository_settings(repository, owner)?, page)
}

fn read_issue(
    repository: &str,
    number: u32,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Option<IssueView>, Error> {
    IssueView::read(reader, &repository_settings(repository, owner)?, number)
}

fn read_pull_request(
    repository: &str,
    number: u32,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Option<PrView>, Error> {
    PrView::read(reader, &repository_settings(repository, owner)?, number)
}

/// The settings `owner` keeps for the Repository the window names `name`, or the defaults for one it keeps
/// none for. A name that isn't one fails.
fn repository_settings(name: &str, owner: &SettingsOwner) -> Result<RepositorySettings, Error> {
    let name = RepositoryName::parse(name)?;
    Ok(owner.view().loaded.settings.repository(name))
}

fn find_items(
    repository: &str,
    typed: &str,
    reader: &StoreReader,
) -> Result<quick_open::QuickOpenFound, Error> {
    quick_open::find(reader, &RepositoryName::parse(repository)?, typed)
}

/// What the Files tab reads besides the database: where the mirrors are, the diff on show, the way to ask
/// the mirrors' worker for what a read found missing, the highlighter, which parses the files, and the
/// language servers.
pub(crate) struct DiffWork {
    pub(crate) folders: Result<Folders, Error>,
    pub(crate) reader: StoreReader,
    pub(crate) diffs: Diffs,
    pub(crate) fetching: Fetching,
    pub(crate) highlighter: Highlighter,
    /// The language servers' thread, which the Definition peek asks.
    pub(crate) definitions: Definitions,
}

impl DiffWork {
    fn sources(&self) -> Result<DiffSources<'_>, Error> {
        let folders = self.folders.as_ref().map_err(Clone::clone)?;
        Ok(DiffSources {
            reader: &self.reader,
            folders,
            diffs: &self.diffs,
            highlighter: &self.highlighter,
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
    /// The merge base the diff starts from, as the Files tab was told.
    base: String,
    start: u32,
    end: u32,
}

impl DiffChunk {
    /// The diff between the commits the chunk names, and its rows.
    fn parse(self) -> Result<(DiffOf, std::ops::Range<u32>), Error> {
        let of = diff_of(&self.repository, self.number, &self.head, &self.base)?;
        Ok((of, self.start..self.end))
    }
}

/// A search in a PR's diff, as the window asks for it when the user types in the Files tab's search (#186).
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct DiffSearch {
    repository: String,
    number: u32,
    /// The commit the diff ends at, as the Files tab was told.
    head: String,
    /// The merge base the diff starts from, as the Files tab was told.
    base: String,
    query: String,
}

impl DiffSearch {
    /// The diff between the commits it names, and the text to find.
    fn parse(self) -> Result<(DiffOf, String), Error> {
        let of = diff_of(&self.repository, self.number, &self.head, &self.base)?;
        Ok((of, self.query))
    }
}

/// Lines of a file at a PR's head that a gap of its diff leaves out, as the window asks for them when the
/// user expands the gap (#182).
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct GapLines {
    repository: String,
    number: u32,
    /// The commit the diff ends at, as the Files tab was told.
    head: String,
    /// The merge base the diff starts from, as the Files tab was told.
    base: String,
    /// The file's place among the diff's files.
    file: u32,
    /// The first line, counted from 1.
    start: u32,
    /// The line after the last.
    end: u32,
}

impl GapLines {
    /// The diff between the commits it names, the file, and its lines. Lines that start at 0 or end before
    /// they start name no lines of a file.
    fn parse(self) -> Result<(DiffOf, u32, std::ops::Range<u32>), Error> {
        if self.start == 0 || self.start > self.end {
            let message = format!(
                "Lines {} to {} aren't lines of a file.",
                self.start, self.end
            );
            return Err(Error::new(ErrorKind::Internal, &message));
        }
        let of = diff_of(&self.repository, self.number, &self.head, &self.base)?;
        Ok((of, self.file, self.start..self.end))
    }
}

/// A line of a file of a PR's diff, as the window asks where it shows when Go to definition lands on it
/// (#444).
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct DiffLine {
    repository: String,
    number: u32,
    /// The commit the diff ends at, as the Files tab was told.
    head: String,
    /// The merge base the diff starts from, as the Files tab was told.
    base: String,
    /// The file's path at `side`: a renamed file's old path at the base.
    path: String,
    side: DiffEnd,
    /// The line's number at `side`, from 1.
    line: u32,
}

impl DiffLine {
    /// The diff between the commits it names, and the line.
    fn parse(self) -> Result<(DiffOf, FileLine), Error> {
        let of = diff_of(&self.repository, self.number, &self.head, &self.base)?;
        let line = FileLine {
            path: self.path,
            side: self.side,
            line: self.line,
        };
        Ok((of, line))
    }
}

/// The diff of PR `number` from `base` to `head`, as the window names it.
fn diff_of(repository: &str, number: u32, head: &str, base: &str) -> Result<DiffOf, Error> {
    let pr = PrAt {
        repository: RepositoryName::parse(repository)?,
        number,
        head: commit(head)?,
    };
    Ok(DiffOf {
        pr,
        base: commit(base)?,
    })
}

/// The commit `sha` names, as the window sent it.
fn commit(sha: &str) -> Result<CommitSha, Error> {
    CommitSha::parse(sha).ok_or_else(|| {
        let message = format!("{sha:?} isn't a commit.");
        Error::new(ErrorKind::NotFound, &message)
    })
}

fn read_pull_request_files(
    repository: &str,
    number: u32,
    work: &DiffWork,
) -> Result<Option<PrFiles>, Error> {
    let name = RepositoryName::parse(repository)?;
    let asks = || work.fetching.mirror_asks(&name);
    let read = PrFiles::read(&work.sources()?, &name, number, &asks)?;
    Ok(read.map(|FilesRead { files, head }| {
        // The user opened the PR: its ref stays, and a head the mirror lacks goes first.
        if let Some(head) = head {
            work.fetching
                .pull_request_opened(name.clone(), number, head);
        }
        // The diff on show is the PR on screen: its Lookup checkout moves to the diff's ends, where text
        // search looks for them, and ty opens its Python files.
        if let PrFiles::Ready {
            head, base, files, ..
        } = &files
        {
            let at = LookupAt {
                base: base.clone(),
                head: head.clone(),
            };
            work.fetching.look_up(name.clone(), at.clone());
            work.definitions.opened(name.clone(), at, files);
        }
        files
    }))
}

fn read_pull_request_row(line: DiffLine, work: &DiffWork) -> Result<Option<LineInDiff>, Error> {
    let (of, line) = line.parse()?;
    PrFiles::row(&work.sources()?, &of, &line)
}

fn read_pull_request_rows(chunk: DiffChunk, work: &DiffWork) -> Result<Vec<DiffRow>, Error> {
    let (of, rows) = chunk.parse()?;
    PrFiles::rows(&work.sources()?, &of, rows)
}

fn find_in_pull_request(search: DiffSearch, work: &DiffWork) -> Result<Found, Error> {
    let (of, query) = search.parse()?;
    PrFiles::find(&work.sources()?, &of, &query)
}

fn read_pull_request_spans(
    chunk: DiffChunk,
    collapsed: &[u32],
    work: &DiffWork,
) -> Result<Vec<Vec<Span>>, Error> {
    let (of, rows) = chunk.parse()?;
    PrFiles::spans(&work.sources()?, &of, rows, collapsed)
}

fn read_pull_request_lines(lines: GapLines, work: &DiffWork) -> Result<Vec<HeadLine>, Error> {
    let (of, file, lines) = lines.parse()?;
    PrFiles::lines(&work.sources()?, &of, file, lines)
}

/// A lookup of where the picked word is defined, as the window asks for it when the user opens the
/// Definition peek (#438): the word at `column` of line `line` of `path`, with the PR's files at `side`.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct DefinitionLookup {
    repository: String,
    /// The merge base the diff on show starts from.
    base: String,
    /// The commit the diff on show ends at.
    head: String,
    /// The file's path at `side`: a renamed file's old path at the base.
    path: String,
    side: DiffEnd,
    /// The line's number at `side`, from 1.
    line: u32,
    /// Where the word starts in the line, in UTF-16 code units.
    column: u32,
    /// The window's number for the lookup, which its cancel names.
    ticket: Ticket,
}

/// Each language server's row in Settings › Advanced, as the servers' thread last wrote them. The
/// `language-servers-changed` event brings each change after. This never waits.
#[tauri::command]
#[specta::specta]
fn language_servers(work: State<'_, DiffWork>) -> Vec<ServerRow> {
    work.definitions.rows()
}

/// Cancels the lookup with `ticket`, as when the user closes the Definition peek while it waits (#440). The
/// lookup then answers that it was cancelled, even if the core takes the cancel before the lookup.
#[tauri::command]
#[specta::specta]
fn cancel_definitions(ticket: Ticket, work: State<'_, DiffWork>) {
    work.definitions.cancel(ticket);
}

/// What became of a query the window asked the text search for (#686).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SearchAsked {
    /// It runs, and the state answers it with `id`.
    Running { id: SearchId },
    /// It can't run, in the regex crate's words, and the query running goes on.
    BadQuery { message: String },
}

/// A query over the code of a PR on screen, as the rail's Search asks it (#686).
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct CodeSearch {
    repository: String,
    /// The merge base the diff the Files tab shows starts from.
    base: String,
    /// The commit that diff ends at.
    head: String,
    query: Query,
}

/// Searches the code of the PR on screen for `asked`'s query, in place of the query running (#686), in the
/// Lookup checkout once it holds the diff the query names. This never waits: the `text-search-changed`
/// event says when to ask `text_search_state` what it found.
#[tauri::command]
#[specta::specta]
fn text_search(asked: CodeSearch, search: State<'_, TextSearch>) -> Result<SearchAsked, Error> {
    let repository = RepositoryName::parse(&asked.repository)?;
    let at = LookupAt {
        base: commit(&asked.base)?,
        head: commit(&asked.head)?,
    };
    Ok(match search.search(repository, at, &asked.query) {
        Ok(id) => SearchAsked::Running { id },
        Err(bad) => SearchAsked::BadQuery {
            message: bad.message,
        },
    })
}

/// What the latest query found, or why it found nothing, as the text search's thread last wrote it. This
/// never waits.
#[tauri::command]
#[specta::specta]
fn text_search_state(search: State<'_, TextSearch>) -> SearchState {
    search.state()
}

/// Cancels query `id` while it runs, as the window wants nothing it finds any more: the box was emptied,
/// the diff it searched went, or the PR closed. The state then goes back to idle.
#[tauri::command]
#[specta::specta]
fn text_search_cancel(id: SearchId, search: State<'_, TextSearch>) {
    search.cancel(id);
}

/// Where `lookup`'s word is defined. `hearing` holds the listeners to what the lookup waits for, as the
/// peek says (#440), and to how the index of a server that answers while it builds it goes (#680).
fn look_up_definition(
    lookup: DefinitionLookup,
    work: &DiffWork,
    hearing: (OnWaiting, OnIndex),
) -> Result<Answer, Error> {
    let at = LookupAt {
        base: commit(&lookup.base)?,
        head: commit(&lookup.head)?,
    };
    let ticket = lookup.ticket;
    let lookup = Lookup {
        repository: RepositoryName::parse(&lookup.repository)?,
        at,
        path: lookup.path,
        side: lookup.side,
        line: lookup.line,
        column: lookup.column,
    };
    work.definitions.look_up(lookup, ticket, hearing)
}

/// The file a hit is in, as the window names it to read its code (#439).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum CodeFile {
    /// A file of the Repository at a commit.
    Repository { path: String, commit: String },
    /// A file outside the Repository, by the path on disk a hit named.
    Outside { path: String },
}

/// Some lines of a hit's file, which the Definition peek shows (#439): lines `start` to `end` of `file`,
/// counted from 1, without `end`.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct DefinitionCode {
    repository: String,
    file: CodeFile,
    start: u32,
    end: u32,
}

impl DefinitionCode {
    /// The Mirror of its Repository, and its file's lines.
    fn parse(self, work: &DiffWork) -> Result<(PathBuf, HitLines), Error> {
        let name = RepositoryName::parse(&self.repository)?;
        let folders = work.folders.as_ref().map_err(Clone::clone)?;
        let file = match self.file {
            CodeFile::Repository { path, commit: sha } => HitFile::Repository {
                path,
                commit: commit(&sha)?,
            },
            CodeFile::Outside { path } => HitFile::Outside {
                path: PathBuf::from(path),
            },
        };
        let lines = self.start..self.end;
        Ok((folders.mirror(&name), HitLines { file, lines }))
    }
}

fn read_definition_code(code: DefinitionCode, work: &DiffWork) -> Result<Code, Error> {
    let (mirror, wanted) = code.parse(work)?;
    work.definitions.code(&mirror, &wanted)
}

/// The spans of the lines `code` names, as the diff's rows have theirs (#455).
fn read_definition_spans(code: DefinitionCode, work: &DiffWork) -> Result<Vec<Vec<Span>>, Error> {
    let (mirror, wanted) = code.parse(work)?;
    work.definitions.spans(&work.highlighter, &mirror, &wanted)
}

/// Opens the page of `item` on GitHub with `show`. It leaves Last seen alone: only Mark Seen moves it.
fn open_github_page(
    repository: &str,
    item: GitHubItem,
    show: impl FnOnce(&WebAddress) -> Result<(), Error>,
) -> Result<(), Error> {
    let name = RepositoryName::parse(repository)?;
    show(&WebAddress::of_item(&name, item))
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Sent when a Repository's stored facts change, so each screen showing them asks again (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct Changed(Change);

/// Sent when a read of an Issue or a PR on screen fails, so a view still waiting for what it says can say
/// why (#189).
#[derive(Clone, Serialize, Type, Event)]
struct ReadFailed(FailedRead);

/// Sent when the open Repository's last sync changes, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct LastSyncChanged(LastSync);

/// Sent when what the queue of syncs is doing changes, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct SyncActivityChanged(Option<SyncActivity>);

/// Sent when an answer from GitHub changes the hour's points, and with nothing when the hour resets, so the
/// status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct HourPointsChanged(Option<HourPoints>);

/// Sent when a mirror's clone moves on, and when it ends, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct CloningChanged(Option<Cloning>);

/// Sent when the worker finds no mirror can be made, and when one can again, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct MirrorProblemChanged(Option<MirrorProblem>);

/// Sent when the first Repository whose mirror waits to hear where it gets its objects changes, so the
/// status bar can ask.
#[derive(Clone, Serialize, Type, Event)]
struct MirrorQuestionChanged(Option<RepositoryName>);

/// Sent when the state of a Repository's mirror changes, so the Repository picker can say (#320).
#[derive(Clone, Serialize, Type, Event)]
struct MirrorStatesChanged(Vec<RepositoryMirror>);

/// Sent when the mirrors' worker starts or stops holding back git while GitHub can't be reached, so a Files
/// tab waiting for the PR's commits can say they wait for the network (#390).
#[derive(Clone, Serialize, Type, Event)]
struct MirrorsHeldChanged(bool);

/// Sent when a check finds a newer version out than it last found, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct NewVersionFound(NewVersion);

/// Sent when PR heads arrive in a Repository's mirror, so a Files tab waiting for one asks again.
#[derive(Clone, Serialize, Type, Event)]
struct HeadsFetched(Change);

/// Sent when a language server's row in Settings › Advanced changes: a check of a new path ends, a server
/// starts or stops, or one has a problem.
#[derive(Clone, Serialize, Type, Event)]
struct LanguageServersChanged(Vec<ServerRow>);

/// Sent when what the latest text search found may have changed: it waits, runs, found more or ended
/// (#686). The window then asks `text_search_state`.
#[derive(Clone, Serialize, Type, Event)]
struct TextSearchChanged;

/// The PR heads that aren't in a Repository's mirror once the worker stopped fetching them, and why.
#[derive(Clone, Serialize, Type)]
struct FailedHeads {
    #[serde(flatten)]
    heads: Change,
    why: Unfetched,
}

/// Sent when the mirror's worker stopped fetching PR heads it didn't bring, so a Files tab waiting for one
/// says why and offers to try again (#233, #213).
#[derive(Clone, Serialize, Type, Event)]
struct HeadsFetchFailed(FailedHeads);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder<R: tauri::Runtime>() -> Builder<R> {
    Builder::<R>::new()
        .commands(collect_commands![
            about,
            // Specta reads the types through `Wry`, and Tauri's handler takes the builder's runtime.
            open_devtools::<tauri::Wry>,
            status,
            last_sync,
            sync_activity,
            hour_points,
            cloning,
            mirror_problem,
            check_mirrors_again,
            mirror_question,
            mirror_states,
            mirrors_held,
            new_version,
            slow_commands::check_clone,
            slow_commands::choose_mirror_source,
            slow_commands::pick_folder,
            slow_commands::pick_program,
            refresh,
            open_issue,
            fetch_issue,
            slow_commands::open_pull_request,
            fetch_pull_request,
            mark_seen,
            mark_unseen,
            settings,
            slow_commands::themes,
            slow_commands::preview_themes,
            turn_window::<tauri::Wry>,
            home,
            token_source,
            pane_layout,
            recent_commands,
            time_running,
            record_measurement,
            record_miss,
            slow_commands::check_repository,
            slow_commands::mirror_cost,
            slow_commands::add_repository,
            slow_commands::put_filter_on_workbench,
            slow_commands::open_repository,
            slow_commands::save_pane_layout,
            slow_commands::remember_command,
            slow_commands::save_theme,
            slow_commands::reread_settings,
            slow_commands::apply_settings,
            slow_commands::workbench,
            slow_commands::inbox,
            slow_commands::issue,
            slow_commands::pull_request,
            slow_commands::pull_request_files,
            slow_commands::pull_request_rows,
            slow_commands::pull_request_spans,
            slow_commands::pull_request_lines,
            slow_commands::pull_request_find,
            slow_commands::pull_request_row,
            slow_commands::quick_open,
            slow_commands::definitions,
            cancel_definitions,
            language_servers,
            slow_commands::open_server_log,
            slow_commands::definition_code,
            slow_commands::definition_spans,
            text_search,
            text_search_state,
            text_search_cancel,
            slow_commands::open_on_github,
            slow_commands::open_link
        ])
        .events(collect_events![
            StatusChanged,
            Changed,
            ReadFailed,
            LastSyncChanged,
            SyncActivityChanged,
            HourPointsChanged,
            CloningChanged,
            MirrorProblemChanged,
            MirrorQuestionChanged,
            MirrorStatesChanged,
            MirrorsHeldChanged,
            NewVersionFound,
            HeadsFetched,
            HeadsFetchFailed,
            LanguageServersChanged,
            TextSearchChanged
        ])
}

#[cfg(test)]
mod tests;
