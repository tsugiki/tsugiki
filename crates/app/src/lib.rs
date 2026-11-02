//! The Tauri app around `tsugiki-core`: commands, events and the window, and nothing else (ADR 0007).

mod startup;

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
pub use startup::run;
use tauri::State;
use tauri_specta::{Builder, Event, collect_commands, collect_events};
use tsugiki_core::status::{Status, StatusBoard};

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about()
}

/// The whole app-wide state, for a window that has just loaded (ADR 0008).
#[tauri::command]
#[specta::specta]
fn status(board: State<'_, Arc<StatusBoard>>) -> Status {
    board.get()
}

/// Sent whenever the app-wide state changes (ADR 0008).
#[derive(Clone, Serialize, Type, Event)]
struct StatusChanged(Status);

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![about, status])
        .events(collect_events![StatusChanged])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;
    use tauri::test::mock_app;
    use tsugiki_core::error::ErrorKind;
    use tsugiki_core::github::live::Https;
    use tsugiki_core::status::connect;

    #[test]
    fn about_answers_with_what_the_core_says() {
        assert_eq!(about(), tsugiki_core::about());
    }

    #[test]
    fn status_answers_with_what_the_board_holds() {
        let board = Arc::new(StatusBoard::new(Box::new(|_| {})));
        // With no token from gh, `connect` never gets as far as using the network.
        let gh_missing = |_: &[&str]| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        connect(&board, Box::new(Https::new()), gh_missing);

        let app = mock_app();
        app.manage(board);
        assert!(matches!(
            status(app.state()),
            Status::Problem { problem } if problem.kind == ErrorKind::Unauthorized
        ));
    }
}
