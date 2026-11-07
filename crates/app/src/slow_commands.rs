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
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::repository::{Home, RepositoryAccess, RepositoryCheck};
use tsugiki_core::session::SessionRequests;
use tsugiki_core::settings::SettingsOwner;
use tsugiki_core::store::StoreReader;
use tsugiki_core::web::{GitHubItem, WebAddress};
use tsugiki_core::workbench::issue_view::IssueView;
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
