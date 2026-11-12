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
use tsugiki_core::diff::{DiffOf, DiffRow, Diffs, Found, HeadLine, Highlighter, PrAt};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::folders::Folders;
use tsugiki_core::github::CommitSha;
use tsugiki_core::highlight::Span;
use tsugiki_core::latest::LatestReader;
use tsugiki_core::measurement::Measurement;
use tsugiki_core::mirror::{
    CloneRefusal, Cloning, MirrorProblem, MirrorRequests, MirrorSource, MirrorSources,
    RepositoryMirror, Unfetched,
};
use tsugiki_core::misses::Miss;
use tsugiki_core::releases::NewVersion;
use tsugiki_core::repository::{RepositoryAccess, RepositoryName};
use tsugiki_core::repository_settings::{MirrorCost, NameCheck, RepositorySettings};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::{Home, LoadedSettings, PaneLayout, SettingsOwner};
use tsugiki_core::status::{Change, FailedRead, LastSync, Status, SyncActivity};
use tsugiki_core::store::StoreReader;
use tsugiki_core::theme::Themes;
use tsugiki_core::version::Version;
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_files::{DiffSources, PrFiles};
use tsugiki_core::workbench::pr_view::PrView;
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

/// Says why the clone in `folder` can't lend its objects to `repository`'s mirror, or nothing if it can.
fn check_named_clone(
    repository: &str,
    folder: &Path,
    work: &RepositoryWork,
) -> Result<Option<CloneRefusal>, Error> {
    let repository = RepositoryName::parse(repository)?;
    Ok(work.mirrors.check_clone(folder, &repository).err())
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
    work.mirrors
        .clone_mirror_next(repository.clone(), Some(source.clone()));
    drop(owner.save_mirror_source(repository, source)?);
    Ok(())
}

/// Opens the system's folder picker and gives the folder the user picked, if they picked one.
pub(crate) type PickFolder = Arc<dyn Fn() -> Option<PathBuf> + Send + Sync>;

/// The folder picker, which the commands share.
pub(crate) struct FolderPicker(PickFolder);

/// Looks at git and the disk again, and tries the clones that waited for them, and returns at once.
#[tauri::command]
#[specta::specta]
fn check_mirrors_again(work: State<'_, RepositoryWork>) {
    work.mirrors.check_again();
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

/// Tells the core the user opened PR `number`, which moves its Last seen, reads its Conversation from GitHub
/// and hands its head to the mirror if the mirror lacks it. The Files tab reads the diff once it shows.
fn open_pull_request(
    repository: &str,
    number: u32,
    requests: &SessionRequests,
    work: &DiffWork,
) -> Result<(), Error> {
    let name = RepositoryName::parse(repository)?;
    requests.open_pull_request(name.clone(), number);
    PrFiles::fetch_missing_head(&work.sources()?, &name, number)
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

/// The theme of each side, light and dark, as CSS variables (#515). The window shows the one for the side
/// Windows is on.
#[tauri::command]
#[specta::specta]
fn themes() -> Result<Themes, Error> {
    Themes::built_in()
}

/// What the window opens on.
#[tauri::command]
#[specta::specta]
fn home(owner: State<'_, SettingsOwner>) -> Home {
    owner.view().home
}

/// How the user laid out the panes on this machine: the Sidebar's and the Workbench list's widths (#187), and
/// whether the Sidebar (#234) and the list (#242) show.
#[tauri::command]
#[specta::specta]
fn pane_layout(owner: State<'_, SettingsOwner>) -> PaneLayout {
    owner.view().panes
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
    requests: &SessionRequests,
) -> Result<NameCheck, Error> {
    owner.check_name(RepositoryName::parse(input)?, &mut |name| {
        requests.check(name)
    })
}

/// Asks GitHub what the mirror of `repository` takes, with the check Add repository makes. The Files tab asks
/// for a Repository Tsugiki has.
fn check_mirror_cost(repository: &str, requests: &SessionRequests) -> Result<MirrorCost, Error> {
    Ok(requests.check(RepositoryName::parse(repository)?)?.mirror)
}

/// The user's choice in Add repository: the Repository, by the name GitHub gave it, what they may do in it,
/// and where its mirror gets its objects. The name stays text until the core parses it, so a name that isn't
/// one gets the same refusal as everywhere else.
#[derive(Debug, Clone, Deserialize, Type)]
pub(crate) struct NewRepository {
    name: String,
    access: RepositoryAccess,
    source: MirrorSource,
}

/// Adds the Repository, and starts its first sync (ADR 0015) and its mirror's clone.
fn add_named_repository(
    repository: NewRepository,
    owner: &SettingsOwner,
    work: &RepositoryWork,
) -> Result<Home, Error> {
    let NewRepository {
        name,
        access,
        source,
    } = repository;
    let view = owner.add_repository(RepositoryName::parse(&name)?, access, source)?;
    work.start(&view.home, &view.mirror_sources);
    Ok(view.home)
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

/// The threads that work on Repositories: the session's sync queue, and the mirrors' one writer.
struct RepositoryWork {
    session: SessionRequests,
    mirrors: MirrorRequests,
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
        self.session.sync(repository.clone());
        self.mirrors
            .clone_mirror_next(repository.name.clone(), source);
    }
}

fn read_workbench(
    repository: &str,
    view: &WorkbenchView,
    owner: &SettingsOwner,
    reader: &StoreReader,
) -> Result<Workbench, Error> {
    let name = RepositoryName::parse(repository)?;
    let settings = owner.view();
    let repository = settings.loaded.settings.repository(name);
    Workbench::read(reader, &repository, view, settings.panes.order)
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

/// What the Files tab reads besides the database: where the mirrors are, the diff on show, the mirrors'
/// worker, which fetches a head a mirror lacks, and the highlighter, which parses the files.
pub(crate) struct DiffWork {
    pub(crate) folders: Result<Folders, Error>,
    pub(crate) reader: StoreReader,
    pub(crate) diffs: Diffs,
    pub(crate) mirrors: MirrorRequests,
    pub(crate) highlighter: Highlighter,
}

impl DiffWork {
    fn sources(&self) -> Result<DiffSources<'_>, Error> {
        let folders = self.folders.as_ref().map_err(Clone::clone)?;
        Ok(DiffSources {
            reader: &self.reader,
            folders,
            diffs: &self.diffs,
            mirrors: &self.mirrors,
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
    PrFiles::read(&work.sources()?, &name, number)
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

/// Sent when a check finds a newer version out than it last found, so the status bar can say.
#[derive(Clone, Serialize, Type, Event)]
struct NewVersionFound(NewVersion);

/// Sent when PR heads arrive in a Repository's mirror, so a Files tab waiting for one asks again.
#[derive(Clone, Serialize, Type, Event)]
struct HeadsFetched(Change);

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
            cloning,
            mirror_problem,
            check_mirrors_again,
            mirror_question,
            mirror_states,
            new_version,
            slow_commands::check_clone,
            slow_commands::choose_mirror_source,
            slow_commands::pick_folder,
            refresh,
            open_issue,
            fetch_issue,
            slow_commands::open_pull_request,
            fetch_pull_request,
            settings,
            themes,
            home,
            pane_layout,
            time_running,
            record_measurement,
            record_miss,
            slow_commands::check_repository,
            slow_commands::mirror_cost,
            slow_commands::add_repository,
            slow_commands::open_repository,
            slow_commands::save_pane_layout,
            slow_commands::workbench,
            slow_commands::issue,
            slow_commands::pull_request,
            slow_commands::pull_request_files,
            slow_commands::pull_request_rows,
            slow_commands::pull_request_spans,
            slow_commands::pull_request_lines,
            slow_commands::pull_request_find,
            slow_commands::open_on_github,
            slow_commands::open_link
        ])
        .events(collect_events![
            StatusChanged,
            Changed,
            ReadFailed,
            LastSyncChanged,
            SyncActivityChanged,
            CloningChanged,
            MirrorProblemChanged,
            MirrorQuestionChanged,
            MirrorStatesChanged,
            NewVersionFound,
            HeadsFetched,
            HeadsFetchFailed
        ])
}

#[cfg(test)]
mod tests;
