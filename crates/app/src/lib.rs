//! The Tauri app around `tsugiki-core`: commands, events and the window, and nothing else (ADR 0007).

mod startup;

pub use startup::run;
use tauri_specta::{Builder, collect_commands};

#[tauri::command]
#[specta::specta]
fn about() -> tsugiki_core::About {
    tsugiki_core::about()
}

/// Every command and event the webview can use. The TypeScript bindings are generated from it (ADR 0008).
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![about])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_answers_with_what_the_core_says() {
        assert_eq!(about(), tsugiki_core::about());
    }
}
