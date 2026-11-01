//! The Tauri app around `tsugiki-core`: commands, events and the window, and nothing else (ADR 0007).

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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use specta_typescript::Typescript;

    use super::*;

    const BINDINGS_HEADER: &str =
        "// Generated from tsugiki-app's commands by `npm run bindings`. Do not edit.";

    /// Where the generated bindings live in the UI's data layer.
    fn bindings_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/data/bindings.ts")
    }

    fn export_bindings(path: &Path) {
        specta_builder()
            .export(Typescript::default().header(BINDINGS_HEADER), path)
            .expect("export bindings");
    }

    /// Fails when the committed bindings differ from what the commands generate.
    /// `TSUGIKI_UPDATE_BINDINGS=1` rewrites them instead; `npm run bindings` does that.
    #[test]
    fn bindings_are_up_to_date() {
        let fresh_path =
            std::env::temp_dir().join(format!("tsugiki-bindings-{}.ts", std::process::id()));
        export_bindings(&fresh_path);
        let fresh = std::fs::read_to_string(&fresh_path).expect("read fresh bindings");
        std::fs::remove_file(&fresh_path).ok();

        if std::env::var("TSUGIKI_UPDATE_BINDINGS").as_deref() == Ok("1") {
            std::fs::write(bindings_path(), &fresh).expect("write bindings");
            return;
        }
        let committed = std::fs::read_to_string(bindings_path()).unwrap_or_default();
        assert!(
            committed == fresh,
            "the TypeScript bindings are out of date: run `npm run bindings` and commit the result"
        );
    }
}
