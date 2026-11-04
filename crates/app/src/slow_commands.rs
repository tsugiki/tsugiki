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
use tsugiki_core::repository::{Home, RepositoryCheck};
use tsugiki_core::session::RepositoryChecks;
use tsugiki_core::settings::SettingsOwner;

use crate::LatestSettings;

/// What adding the Repository the user typed would load. A check can wait out a stuck request to GitHub, so
/// it waits on a thread meant for blocking, and leaves the threads that answer the window's other commands
/// free.
#[tauri::command]
#[specta::specta]
pub(crate) async fn check_repository(
    input: String,
    checks: State<'_, RepositoryChecks>,
) -> Result<RepositoryCheck, Error> {
    let checks = checks.inner().clone();
    spawn_blocking(move || crate::check_typed_repository(&input, &checks))
        .await
        .unwrap_or_else(|cause| {
            log::error!("A check of a Repository stopped: {cause}");
            Err(Error::new(
                ErrorKind::Internal,
                "Tsugiki couldn't finish that check.",
            ))
        })
}

/// Adds a Repository the user has seen the check for, and says what the window opens on now. This waits on
/// a small file and never on GitHub.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn add_repository(
    name: &str,
    owner: State<'_, SettingsOwner>,
    latest: State<'_, LatestSettings>,
) -> Result<Home, Error> {
    crate::add_named_repository(name, &owner, &latest)
}
