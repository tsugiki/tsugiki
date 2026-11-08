//! Commands that can wait, so they stay off the thread that draws the window. They only call what `lib.rs`
//! tests.
//!
//! This file is out of the coverage gate. Tauri's macro writes an async block on each attribute's line, and
//! the report calls it unrun even though `lib.rs`'s IPC tests can only get their answers through it.

#![expect(
    clippy::unreachable,
    reason = "Tauri's macro writes it in a compile-time check on `async fn` commands, which never runs"
)]

use tauri::State;
use tauri::async_runtime::spawn_blocking;
use tsugiki_core::diff::{DiffRow, HeadLine};
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::highlight::Span;
use tsugiki_core::repository::{Home, RepositoryAccess, RepositoryCheck};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::{PaneWidths, SettingsOwner};
use tsugiki_core::store::StoreReader;
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::workbench::issue_view::IssueView;
use tsugiki_core::workbench::pr_files::PrFiles;
use tsugiki_core::workbench::pr_view::PrView;
use tsugiki_core::workbench::{Workbench, WorkbenchView};

/// What adding the Repository the user typed would load. A check can wait out a stuck request to GitHub, so
/// it waits on a thread meant for blocking, and leaves the threads that answer the window's other commands
/// free.
#[tauri::command]
#[specta::specta]
pub(crate) async fn check_repository(
    input: String,
    requests: State<'_, SessionRequests>,
) -> Result<RepositoryCheck, Error> {
    let requests = requests.inner().clone();
    spawn_blocking(move || crate::check_typed_repository(&input, &requests))
        .await
        .unwrap_or_else(|cause| {
            log::error!("A check of a Repository stopped: {cause}");
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
    name: &str,
    access: RepositoryAccess,
    owner: State<'_, SettingsOwner>,
    work: State<'_, crate::RepositoryWork>,
) -> Result<Home, Error> {
    crate::add_named_repository(name, access, &owner, &work)
}

/// Saves how wide the user dragged the Sidebar and the Workbench list in `machine.toml`, and says the widths
/// the window keeps now. This waits on a small file.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn save_pane_widths(
    widths: PaneWidths,
    owner: State<'_, SettingsOwner>,
) -> Result<PaneWidths, Error> {
    Ok(owner.save_panes(widths)?.panes)
}

/// The rows of a view of a Repository's Workbench, with what its Sidebar shows. This waits
/// on the database and never on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn workbench(
    repository: &str,
    view: WorkbenchView,
    owner: State<'_, SettingsOwner>,
    reader: State<'_, StoreReader>,
) -> Result<Workbench, Error> {
    crate::read_workbench(repository, &view, &owner, &reader)
}

/// The Issue view of Issue `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and
/// never on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn issue(
    repository: &str,
    number: u32,
    owner: State<'_, SettingsOwner>,
    reader: State<'_, StoreReader>,
) -> Result<Option<IssueView>, Error> {
    crate::read_issue(repository, number, &owner, &reader)
}

/// The PR view of PR `number`, or nothing if Tsugiki doesn't hold it. This waits on the database and never
/// on GitHub (ADR 0018).
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request(
    repository: &str,
    number: u32,
    owner: State<'_, SettingsOwner>,
    reader: State<'_, StoreReader>,
) -> Result<Option<PrView>, Error> {
    crate::read_pull_request(repository, number, &owner, &reader)
}

/// Opens an Issue or a PR on GitHub in the system browser.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_on_github(
    repository: &str,
    item: GitHubItem,
    requests: State<'_, SessionRequests>,
) -> Result<(), Error> {
    crate::open_github_page(repository, item, &requests, show_in_browser)
}

/// Opens a link from a description or a comment in the system browser. Tsugiki never navigates its own window
/// to a web page.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn open_link(address: &str) -> Result<(), Error> {
    show_in_browser(&WebAddress::parse(address)?)
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
    work: State<'_, crate::DiffWork>,
    reader: State<'_, StoreReader>,
) -> Result<Option<PrFiles>, Error> {
    crate::read_pull_request_files(repository, number, &work, &reader)
}

/// Rows of a PR's diff, which come from the diff on show unless the user has opened another PR since.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_rows(
    chunk: crate::DiffChunk,
    work: State<'_, crate::DiffWork>,
    reader: State<'_, StoreReader>,
) -> Result<Vec<DiffRow>, Error> {
    crate::read_pull_request_rows(chunk, &work, &reader)
}

/// The spans that highlight the code in a chunk of a PR's diff, which the Files tab asks for with its rows
/// and paints when they come. `opened` names the Generated files it shows open, by their place among the
/// files: the rows of the others get none.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_spans(
    chunk: crate::DiffChunk,
    opened: Vec<u32>,
    work: State<'_, crate::DiffWork>,
    reader: State<'_, StoreReader>,
) -> Result<Vec<Vec<Span>>, Error> {
    crate::read_pull_request_spans(chunk, &opened, &work, &reader)
}

/// Where the code of a PR's diff holds the text the user searches for, whatever the case, in every row:
/// the window holds only those it shows. It asks GitHub for nothing.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_find(
    search: crate::DiffSearch,
    work: State<'_, crate::DiffWork>,
    reader: State<'_, StoreReader>,
) -> Result<tsugiki_core::diff::Found, Error> {
    crate::find_in_pull_request(search, &work, &reader)
}

/// The lines a gap of a PR's diff leaves out, read from the file at the PR's head in its mirror with their
/// spans, which the Files tab shows when the user expands the gap. It asks GitHub for nothing. Those past
/// the file's end don't come.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn pull_request_lines(
    lines: crate::GapLines,
    work: State<'_, crate::DiffWork>,
    reader: State<'_, StoreReader>,
) -> Result<Vec<HeadLine>, Error> {
    crate::read_pull_request_lines(lines, &work, &reader)
}
