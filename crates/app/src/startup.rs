//! Opens Tsugiki's window. Measuring this takes a real window, so it is left out of the
//! coverage gate and the `tauri-driver` smoke tests cover it instead (ADR 0009).

use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;
use tauri_specta::Event;
use tsugiki_core::error::{Error, ErrorKind};
use tsugiki_core::folders::{Folders, Os};
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::status::{GitHubAccess, StatusBoard, start};

use crate::{StatusChanged, specta_builder};

/// How often Tsugiki asks GitHub whether it is still there, at about one point of the hourly 5,000 each.
const REFRESH: Duration = Duration::from_secs(60);

/// Finds the folders from this program's real path and environment.
fn find_folders() -> Result<Folders, Error> {
    #[expect(
        clippy::map_err_ignore,
        reason = "the cause goes to the local log once #58 adds one"
    )]
    let exe = std::env::current_exe().map_err(|_| {
        Error::new(
            ErrorKind::Internal,
            "Tsugiki can't tell where it is running from",
        )
    })?;
    Folders::find(Os::CURRENT, &exe, &|variable| std::env::var_os(variable))
}

/// Starts Tsugiki's window.
#[expect(
    clippy::expect_used,
    reason = "without a window the user has nowhere to read an error, so a failure ends the program"
)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let handle = app.handle().clone();
            let board = Arc::new(StatusBoard::new(Box::new(move |status| {
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "a window still loading asks `status()` when it is done; \
                              any other failure goes to the local log once #58 adds one"
                )]
                let _ = StatusChanged(status.clone()).emit(&handle);
            })));
            let _ = app.manage(Arc::clone(&board));
            // The window opens without waiting on the disk, gh or the network (ADR 0017). `start` shows its
            // own failures on the board, so nothing needs the thread's handle.
            let _ = std::thread::spawn(move || {
                let access = GitHubAccess {
                    transport: Box::new(Https::new()),
                    run_gh,
                };
                start(&board, find_folders(), access, REFRESH);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tsugiki failed to start");
}
