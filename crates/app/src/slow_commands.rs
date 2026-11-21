//! Commands that can wait, so they stay off the thread that draws the window. They only call what `lib.rs`
//! tests.
//!
//! This file is out of the coverage gate. Tauri's macro writes an async block on each attribute's line, and
//! the report calls it unrun even though `lib.rs`'s IPC tests can only get their answers through it.

#![expect(
    clippy::unreachable,
    reason = "Tauri's macro writes it in a compile-time check on `async fn` commands, which never runs"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::State;
use tauri::async_runtime::spawn_blocking;
use tauri::ipc::Channel;
use tsugiki_core::calls::{Calls, ItemFetch, PrFiles};
use tsugiki_core::definitions::{Answer, Index, ServerName, Waiting};
use tsugiki_core::diff::{DiffRow, HeadLine};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::highlight::Span;
use tsugiki_core::mirror::CloneRefusal;
use tsugiki_core::mirror_source::MirrorSource;
use tsugiki_core::repository_settings::{MirrorCost, NameCheck};
use tsugiki_core::settings::{
    AppearanceSettings, CommandId, Home, LoadedSettings, PaneLayout, SettingsChanges, SettingsOwner,
};
use tsugiki_core::theme::{ThemeLibrary, ThemePick, Themes};
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::workbench::inbox::{Inbox, InboxPage};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_view::PrView;
use tsugiki_core::workbench::quick_open::QuickOpenFound;
use tsugiki_core::workbench::{Workbench, WorkbenchView};

/// Whether Tsugiki has the Repository the user typed, and if not, what adding it would load.
#[tauri::command]
#[specta::specta]
pub(crate) async fn check_repository(
    input: String,
    calls: State<'_, Calls>,
) -> Result<NameCheck, Error> {
    let calls = calls.inner().clone();
    check_aside(move || crate::check_typed_repository(&input, &calls)).await
}

/// Asks GitHub the disk and the time the mirror of `repository` takes, for the user to choose where it gets
/// its objects.
#[tauri::command]
#[specta::specta]
pub(crate) async fn mirror_cost(
    repository: String,
    calls: State<'_, Calls>,
) -> Result<MirrorCost, Error> {
    let calls = calls.inner().clone();
    check_aside(move || crate::check_mirror_cost(&repository, &calls)).await
}

/// Fetches Issue or PR `number`, which the database lacks, ahead of the sync's queued work, and says
/// whether it was an Issue or a PR, which the database now holds, or why nothing came (#981).
#[tauri::command]
#[specta::specta]
pub(crate) async fn fetch_item(
    repository: String,
    number: u32,
    calls: State<'_, Calls>,
) -> Result<ItemFetch, Error> {
    let calls = calls.inner().clone();
    check_aside(move || crate::fetch_unstored(&repository, number, &calls)).await
}

/// Runs `check`, which asks GitHub, on a thread meant for blocking. A check can wait out a stuck request, and
/// this leaves the threads that answer the window's other commands free.
async fn check_aside<T: Send + 'static>(
    check: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    spawn_blocking(check).await.unwrap_or_else(|cause| {
        log::error!("A check on GitHub stopped: {cause}");
        Err(Error::new(
            ErrorKind::Internal,
            "Tsugiki couldn't finish that check.",
        ))
    })
}

/// Adds a Repository the user has seen the check for, starts its first sync and its mirror's clone, and says
/// what the window opens on now. This waits on a small file and never on GitHub or git.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn add_repository(
    repository: crate::NewRepository,
    calls: State<'_, Calls>,
) -> Result<Home, Error> {
    crate::add_named_repository(repository, &calls)
}

/// Puts the Filters of a Repository that search a query on the Workbench, or takes them off it, in
/// `settings.toml` (#819), and the settings owner hands the session the settings, so Reach follows them.
/// The window then asks for the Workbench again. This waits on a small file and never on GitHub or git.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn put_filter_on_workbench(
    change: crate::FilterSwitch,
    owner: State<'_, SettingsOwner>,
) -> Result<(), Error> {
    crate::switch_filter(change, &owner)
}

/// Makes `name` the open Repository, starts its sync and puts its mirror's clone next, and says what the
/// window shows now. This waits on a small file and never on GitHub or git.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_repository(name: &str, calls: State<'_, Calls>) -> Result<Home, Error> {
    crate::open_named_repository(name, &calls)
}

/// Says why the clone in `folder` can't lend its objects to `repository`'s mirror, or nothing if it can.
/// This waits on git commands that only read the clone.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn check_clone(
    repository: &str,
    folder: PathBuf,
    calls: State<'_, Calls>,
) -> Result<Option<CloneRefusal>, Error> {
    crate::check_named_clone(repository, &folder, &calls)
}

/// Starts `repository`'s clone ahead of the others, with its objects from `source`, and saves the answer in
/// `machine.toml`. This waits on a small file.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn choose_mirror_source(
    repository: &str,
    source: MirrorSource,
    calls: State<'_, Calls>,
) -> Result<(), Error> {
    crate::choose_named_source(repository, source, &calls)
}

/// Opens the system's folder picker, and gives the folder the user picked, or nothing if they cancelled. It
/// waits on the user, so it waits on a thread meant for blocking.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_folder(
    picker: State<'_, crate::Picker>,
) -> Result<Option<PathBuf>, Error> {
    pick(&picker, crate::Pick::Folder).await
}

/// Opens the system's file picker for a language server's program, as Browse… on its row in Settings ›
/// Advanced does, and gives the file the user picked, or nothing if they cancelled.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_program(
    picker: State<'_, crate::Picker>,
) -> Result<Option<PathBuf>, Error> {
    pick(&picker, crate::Pick::Program).await
}

/// Waits for the user to pick `what`, on a thread meant for blocking.
async fn pick(picker: &crate::Picker, what: crate::Pick) -> Result<Option<PathBuf>, Error> {
    let pick = Arc::clone(&picker.0);
    spawn_blocking(move || pick(what)).await.map_err(|cause| {
        log::error!("The picker stopped: {cause}");
        Error::new(ErrorKind::Internal, "Tsugiki couldn't open the picker.")
    })
}

/// Saves how the user laid out the panes in `machine.toml`, and says the layout the window keeps now. This
/// waits on a small file.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn save_pane_layout(
    layout: PaneLayout,
    owner: State<'_, SettingsOwner>,
) -> Result<PaneLayout, Error> {
    Ok(owner.save_panes(layout)?.panes)
}

/// Notes in `machine.toml` that the user ran `command` from the command palette, so the palette lists it
/// first, and says the palette's recent commands now, most recently run first (#556). This waits on a small
/// file.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn remember_command(
    command: CommandId,
    owner: State<'_, SettingsOwner>,
) -> Result<Vec<CommandId>, Error> {
    owner.remember_command(command)
}

/// Every theme of each side, light and dark, as CSS variables (#515), with the theme the settings choose for
/// each side, and the theme in force (#755). Following the system, the window shows the chosen theme for the
/// side the system is on. The theme picker lists the others, and a theme it saved is the chosen one from
/// then on (#518). The user's theme files come after the built-in ones, read again from the `themes` folder
/// on each ask, so the window asks when it starts, when the picker opens and on Reload themes (#519). This
/// waits on the folder, which may sit on a synced drive. The page then asks `turn_window` to turn the
/// window to the side of the theme it shows.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn themes(owner: State<'_, SettingsOwner>) -> Result<Themes, Error> {
    let view = owner.view();
    view.themes(&view.loaded.settings.appearance)
}

/// The themes `themes` would answer with were `[appearance]` to hold `appearance`, worked out by the same
/// rules, so Settings › General shows its rows on the window as Apply will (#757). This waits on the
/// `themes` folder, as `themes` does.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn preview_themes(
    appearance: AppearanceSettings,
    owner: State<'_, SettingsOwner>,
) -> Result<Themes, Error> {
    owner.view().themes(&appearance)
}

/// Saves the theme picker's choice to `[appearance] theme` in `settings.toml`, without Settings' Apply (#518,
/// #756): follow the system, or a Theme, which may be a file of the `themes` folder (#519). The window shows
/// it already. This waits on small files.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn save_theme(pick: ThemePick, owner: State<'_, SettingsOwner>) -> Result<(), Error> {
    let library = ThemeLibrary::read(&owner.view().themes_folder)?;
    owner.save_theme(library.choice(pick)?).map(drop)
}

/// Reads both settings files again as the Settings dialog opens, so a hand edit made since start-up shows in
/// it and takes effect (ADR 0014). The settings owner hands the session each Repository's settings the edit
/// changed, as it does after every change. This waits on small files.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn reread_settings(calls: State<'_, Calls>) -> Result<LoadedSettings, Error> {
    calls.reread_settings()
}

/// Saves the tables the Settings dialog's Apply changed to `settings.toml`, and the language servers to
/// `machine.toml`, over what the files say now and keeping the rest of them, and answers with the settings in
/// force after. The window previews them already. A server whose path changed is checked after, and its row
/// in Settings › Advanced says what the check found. This waits on small files.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn apply_settings(
    changes: SettingsChanges,
    calls: State<'_, Calls>,
) -> Result<LoadedSettings, Error> {
    calls.apply_settings(changes)
}

/// The rows of a view of a Repository's Workbench, with what its Sidebar shows, in the order `machine.toml`
/// keeps for the list (#592). This waits on the database and never on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn workbench(
    repository: &str,
    view: WorkbenchView,
    calls: State<'_, Calls>,
) -> Result<Workbench, Error> {
    crate::read_workbench(repository, &view, &calls)
}

/// Page `page` of a Repository's Inbox: its open Issues, each with its open PRs, and its open Standalone
/// PRs, the latest update first. This waits on the database and never on GitHub (ADR 0018), and queues
/// nothing, so the Inbox adds nothing to Reach.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn inbox(
    repository: &str,
    page: InboxPage,
    calls: State<'_, Calls>,
) -> Result<Inbox, Error> {
    crate::read_inbox(repository, page, &calls)
}

/// The Issue view of Issue `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and
/// never on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn issue(
    repository: &str,
    number: u32,
    calls: State<'_, Calls>,
) -> Result<Option<IssueView>, Error> {
    crate::read_issue(repository, number, &calls)
}

/// Tells the core the user opened a PR, and returns once git has said whether the mirror holds its head.
/// GitHub's answer comes later.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_pull_request(
    repository: &str,
    number: u32,
    calls: State<'_, Calls>,
) -> Result<(), Error> {
    crate::open_pull_request(repository, number, &calls)
}

/// The PR view of PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never
/// on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request(
    repository: &str,
    number: u32,
    calls: State<'_, Calls>,
) -> Result<Option<PrView>, Error> {
    crate::read_pull_request(repository, number, &calls)
}

/// The Issues and PRs of `repository` that quick open finds for what the user `typed`, best first, with the
/// one the text names by its number apart. This waits on the database and never on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn quick_open(
    repository: &str,
    typed: &str,
    calls: State<'_, Calls>,
) -> Result<QuickOpenFound, Error> {
    crate::find_items(repository, typed, &calls)
}

/// Opens an Issue or a PR on GitHub in the system browser.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_on_github(repository: &str, item: GitHubItem) -> Result<(), Error> {
    crate::open_github_page(repository, item, show_in_browser)
}

/// Opens a link from a description or a comment in the system browser. Tsugiki never navigates its own window
/// to a web page.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_link(address: &str) -> Result<(), Error> {
    show_in_browser(&WebAddress::parse(address)?)
}

/// Opens the log language server `server` writes for `repository` in the system's program for it, as Open
/// log on the server's row in Settings › Advanced does.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_server_log(
    repository: &str,
    server: ServerName,
    calls: State<'_, Calls>,
) -> Result<(), Error> {
    open_file(&crate::server_log_of(repository, server, &calls)?)
}

/// Starting a program can take a moment, so this runs on the commands that can wait. No test calls it: it
/// would open a real window over the user's work.
fn open_file(file: &Path) -> Result<(), Error> {
    open::that_detached(file).map_err(|cause| {
        log::error!("{} didn't open: {cause}", file.display());
        Error::new(ErrorKind::Internal, "Tsugiki couldn't open the log.")
    })
}

/// Starting a browser can take a moment, so this runs on the commands that can wait. No test calls it: it
/// would open a real browser over the user's work.
fn show_in_browser(address: &WebAddress) -> Result<(), Error> {
    open::that_detached(address.as_str()).map_err(|cause| {
        log::error!("The system browser didn't open: {cause}");
        Error::new(
            ErrorKind::Internal,
            "Tsugiki couldn't open the system browser.",
        )
    })
}

/// What a PR's Files tab shows: its diff's files, or why they aren't there yet. A diff that isn't on show
/// waits on git, and never on GitHub.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_files(
    repository: &str,
    number: u32,
    calls: State<'_, Calls>,
) -> Result<Option<PrFiles>, Error> {
    crate::read_pull_request_files(repository, number, &calls)
}

/// Rows of a PR's diff, which come from the diff on show unless the user has opened another PR since.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_rows(
    chunk: crate::DiffChunk,
    calls: State<'_, Calls>,
) -> Result<Vec<DiffRow>, Error> {
    crate::read_pull_request_rows(chunk, &calls)
}

/// The spans that highlight the code in a chunk of a PR's diff, which the Files tab asks for with its rows
/// and paints when they come. `collapsed` names the files it shows collapsed to their headers, by their
/// place among the files: their rows get none.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_spans(
    chunk: crate::DiffChunk,
    collapsed: Vec<u32>,
    calls: State<'_, Calls>,
) -> Result<Vec<Vec<Span>>, Error> {
    crate::read_pull_request_spans(chunk, &collapsed, &calls)
}

/// Where the code of a PR's diff holds the text the user searches for, whatever the case, in every row:
/// the window holds only those it shows. It asks GitHub for nothing.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_find(
    search: crate::DiffSearch,
    calls: State<'_, Calls>,
) -> Result<tsugiki_core::diff::Found, Error> {
    crate::find_in_pull_request(search, &calls)
}

/// The row of a PR's diff that shows a line of one of its files (#444), or else the line at the head, which
/// a gap or the end of the file hides and the window opens (#445). Go to definition scrolls there. It reads
/// the diff on show, or reads it again from the mirror.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_row(
    line: crate::DiffLine,
    calls: State<'_, Calls>,
) -> Result<Option<tsugiki_core::workbench::pr_files::LineInDiff>, Error> {
    crate::read_pull_request_row(line, &calls)
}

/// Where the picked word is defined, which the Definition peek shows (#438). It waits on ty, which reads the
/// Lookup checkout, and on ty's start if it hasn't started, and never on GitHub, so it works offline at any
/// commit the mirror holds. It cancels the lookup running, if one is. `waiting` hears what it waits for:
/// ty to start, or to answer (#440). `index` hears how far clangd has got with its index, if it answers
/// while it builds it, and what clangd names once the core has asked it again with the index built (#680).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn definitions(
    lookup: crate::DefinitionLookup,
    waiting: Channel<Waiting>,
    index: Channel<Index>,
    calls: State<'_, Calls>,
) -> Result<Answer, Error> {
    let heard = Box::new(move |step| {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the window has stopped listening, and then nobody wants to hear it"
        )]
        let _ = waiting.send(step);
    });
    let indexed = Box::new(move |news| {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the window has stopped listening, and then nobody wants to hear it"
        )]
        let _ = index.send(news);
    });
    crate::look_up_definition(lookup, &calls, (heard, indexed))
}

/// Some lines of the file a hit is in, which the Definition peek shows (#439): a file of the Repository at
/// the hit's commit, read from the mirror, or one outside it that a language server named. It never waits
/// for a lookup, nor for GitHub.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn definition_code(
    code: crate::DefinitionCode,
    calls: State<'_, Calls>,
) -> Result<tsugiki_core::definitions::Code, Error> {
    crate::read_definition_code(code, &calls)
}

/// The spans that highlight the lines `definition_code` reads, from the file parsed whole, which the
/// Definition peek and the file view paint once they come (#455). A file the highlighter doesn't keep waits
/// for its parse, so the window shows the lines first. It never waits for a lookup, nor for GitHub.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn definition_spans(
    code: crate::DefinitionCode,
    calls: State<'_, Calls>,
) -> Result<Vec<Vec<Span>>, Error> {
    crate::read_definition_spans(code, &calls)
}

/// The lines a gap of a PR's diff leaves out, read from the file at the PR's head in its mirror with their
/// spans, which the Files tab shows when the user expands the gap. It asks GitHub for nothing. Those past
/// the file's end don't come.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_lines(
    lines: crate::GapLines,
    calls: State<'_, Calls>,
) -> Result<Vec<HeadLine>, Error> {
    crate::read_pull_request_lines(lines, &calls)
}
