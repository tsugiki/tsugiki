//! Opens Tsugiki's window. Measuring this takes a real window, so it is left out of the
//! coverage gate and the `tauri-driver` smoke tests cover it instead (ADR 0009).

use std::time::Duration;

use tauri::{Manager, RunEvent};
use tauri_specta::Event;
use tsugiki_core::folders::Folders;
use tsugiki_core::github::GitHubAccess;
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::logfile;
use tsugiki_core::session::Session;
use tsugiki_core::status::StatusBoard;

use crate::{LatestStatus, StatusChanged, specta_builder};

/// How often Tsugiki asks GitHub whether it is still there, at about one point of the hourly 5,000 each.
const REFRESH: Duration = Duration::from_secs(60);

/// How long the process waits for the session to close the database after its window has gone. A request to
/// GitHub, a call to `gh` or a migration can use all of it. Flushing the log can then add up to another
/// second.
const QUIT_PATIENCE: Duration = Duration::from_secs(1);

/// Starts Tsugiki's window.
#[expect(
    clippy::expect_used,
    reason = "without a window the user has nowhere to read an error, so a failure ends the program"
)]
pub fn run() {
    let folders = Folders::current();
    if let Ok(found) = &folders {
        logfile::start(found.log());
    }
    log_panics();
    let builder = specta_builder();
    let app = tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .build(tauri::generate_context!())
        .expect("Tsugiki failed to start");
    builder.mount_events(&app);
    assert!(
        app.manage(LatestStatus::default()),
        "nothing else manages the latest status"
    );
    let handle = app.handle().clone();
    let board = StatusBoard::new(Box::new(move |status| {
        // Before the event: a window that starts listening just too late for it then asks `status()`, and has
        // to get this status.
        handle.state::<LatestStatus>().set(status);
        // A window still loading asks `status()` when it is done, so it doesn't need this event.
        if let Err(cause) = StatusChanged(status.clone()).emit(&handle) {
            log::warn!("Tsugiki can't tell the window about a new status: {cause}");
        }
    }));
    let access = GitHubAccess {
        transport: Box::new(Https::new()),
        run_gh,
    };
    let session = Session::start(board, folders, access, REFRESH);
    app.run(move |_, event| {
        if let RunEvent::Exit = event {
            session.stop(QUIT_PATIENCE);
            log::logger().flush();
        }
    });
}

/// A panic prints to a console a released app doesn't have, so its message and place also go to the log.
fn log_panics() {
    let print = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        log::error!("{panic}");
        log::logger().flush();
        print(panic);
    }));
}
