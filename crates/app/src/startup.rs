//! Opens Tsugiki's window. Measuring this takes a real window, so it is left out of the
//! coverage gate and the `tauri-driver` smoke tests cover it instead (ADR 0009).

use crate::specta_builder;

/// Starts Tsugiki's window.
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Tsugiki failed to start");
}
