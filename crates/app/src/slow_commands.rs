//! Commands that wait on GitHub or the disk, so Tauri runs them off the thread that draws the window. They
//! only call what `lib.rs` tests.
//!
//! This file is out of the coverage gate. Tauri's macro writes an async block on each attribute's line, and
//! the report calls it unrun even though `lib.rs`'s IPC tests can only get their answers through it.

use tauri::State;
use tsugiki_core::error::Error;
use tsugiki_core::repository::{Home, RepositoryCheck};
use tsugiki_core::session::RepositoryChecks;

use crate::OwnedSettings;

/// What adding the Repository the user typed would load.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn check_repository(
    input: &str,
    checks: State<'_, RepositoryChecks>,
) -> Result<RepositoryCheck, Error> {
    crate::check_typed_repository(input, &checks)
}

/// Adds a Repository the user has seen the check for, and says what the window opens on now.
#[tauri::command(async)]
#[specta::specta]
pub(crate) fn add_repository(name: &str, owned: State<'_, OwnedSettings>) -> Result<Home, Error> {
    crate::add_named_repository(name, &owned)
}
