//! The Tauri app around `tsugiki-core`: commands, events, the window and a read-only copy of the core's
//! status for those commands (ADR 0007).

mod startup;

use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::settings::LoadedSettings;
use tsugiki_core::status::Status;

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about()
}

/// A read-only copy of the session's status. It lets `status` answer while the session waits on GitHub, since
/// the window doesn't wait on the network (ADR 0017).
#[derive(Default)]
struct LatestStatus(Mutex<Status>);

impl LatestStatus {
    fn get(&self) -> Status {
        self.lock().clone()
    }

    fn set(&self, status: &Status) {
        self.lock().clone_from(status);
    }

    /// Only this type holds the lock, and it calls out to nothing while it does, so poisoning should never
    /// happen. If it does, the status is still whole, because `set` only ever replaces it.
    fn lock(&self) -> MutexGuard<'_, Status> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The whole app-wide state, for a window that has just loaded (ADR 0008).
#[tauri::command]
#[specta::specta]
fn status(latest: State<'_, LatestStatus>) -> Status {
    latest.get()
}

/// The settings Tsugiki read when it started, and what was wrong with their files (ADR 0014).
#[tauri::command]
#[specta::specta]
fn settings(loaded: State<'_, LoadedSettings>) -> LoadedSettings {
    loaded.inner().clone()
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![about, status, settings])
        .events(collect_events![StatusChanged])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;
    use tauri::test::mock_app;

    #[test]
    fn about_answers_with_what_the_core_says() {
        assert_eq!(about(), tsugiki_core::about());
    }

    #[test]
    fn status_says_connecting_until_the_session_says_otherwise() {
        let app = mock_app();
        assert!(app.manage(LatestStatus::default()));
        assert_eq!(status(app.state()), Status::Connecting);
    }

    #[test]
    fn status_answers_with_the_latest_status() {
        let app = mock_app();
        assert!(app.manage(LatestStatus::default()));
        app.state::<LatestStatus>().set(&Status::Offline);
        assert_eq!(status(app.state()), Status::Offline);
    }

    #[test]
    fn settings_answers_with_what_was_read_at_start() {
        let app = mock_app();
        let loaded = LoadedSettings {
            problems: vec!["settings.toml, line 3: expected a value".to_owned()],
            ..LoadedSettings::default()
        };
        assert!(app.manage(loaded.clone()));
        assert_eq!(settings(app.state()), loaded);
    }

    #[test]
    fn a_panic_while_holding_the_lock_doesnt_stop_the_status_working() {
        let latest = LatestStatus::default();
        std::thread::scope(|scope| {
            let holder = scope.spawn(|| {
                let _guard = latest.0.lock();
                panic!("a panic while holding the lock");
            });
            assert!(holder.join().is_err());
        });
        assert!(latest.0.is_poisoned());

        assert_eq!(latest.get(), Status::Connecting);
        latest.set(&Status::Offline);
        assert_eq!(latest.get(), Status::Offline);
    }
}
