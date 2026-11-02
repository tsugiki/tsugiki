//! Opens Tsugiki's window. Measuring this takes a real window, so it is left out of the
//! coverage gate and the `tauri-driver` smoke tests cover it instead (ADR 0009).

use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;
use tauri_specta::Event;
use tsugiki_core::github::live::{Https, run_gh};
use tsugiki_core::status::{StatusBoard, connect, refresh};

use crate::{StatusChanged, specta_builder};

/// How often Tsugiki asks GitHub whether it is still there, at about one point of the hourly 5,000 each.
const REFRESH: Duration = Duration::from_secs(60);

/// Starts Tsugiki's window.
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let handle = app.handle().clone();
            let board = Arc::new(StatusBoard::new(Box::new(move |status| {
                // A window that misses this asks `status()` when it loads.
                let _ = StatusChanged(status.clone()).emit(&handle);
            })));
            app.manage(Arc::clone(&board));
            // The window opens without waiting on gh or the network (ADR 0017).
            std::thread::spawn(move || {
                let Some(github) = connect(&board, Box::new(Https::new()), run_gh) else {
                    return;
                };
                loop {
                    std::thread::sleep(REFRESH);
                    refresh(&board, &github);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tsugiki failed to start");
}
