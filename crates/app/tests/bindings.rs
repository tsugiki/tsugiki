//! Fails when the committed TypeScript bindings differ from what the commands generate (ADR 0008), or the
//! committed key table from the core's (#548). `TSUGIKI_UPDATE_BINDINGS=1` rewrites them instead; `npm run
//! bindings` does that. `npm run cargo-test` skips both tests by the `bindings_are_up_to_date` in their
//! names, and `npm run stale-bindings` runs them.

use specta_typescript::Typescript;

const HEADER: &str = "// Generated from tsugiki-app's commands by `npm run bindings`. Do not edit.";

/// Writes `fresh` to `path`, from the top of the repo, under `TSUGIKI_UPDATE_BINDINGS=1`, and otherwise fails
/// unless the committed file says the same.
fn compare_or_update(path: &str, fresh: &str) {
    tsugiki_test_git::compare_or_update(path, fresh, "TSUGIKI_UPDATE_BINDINGS", "npm run bindings");
}

#[test]
fn bindings_are_up_to_date() {
    let folder = tsugiki_test_git::temp_path("bindings");
    std::fs::create_dir_all(&folder).expect("create the folder for the fresh bindings");
    let fresh_path = folder.join("bindings.ts");
    tsugiki_app::specta_builder::<tauri::Wry>()
        .export(Typescript::default().header(HEADER), &fresh_path)
        .expect("export bindings");
    let fresh = std::fs::read_to_string(&fresh_path).expect("read fresh bindings");
    compare_or_update("ui/src/lib/data/bindings.ts", &fresh);
}

/// The key table goes in a file of its own, apart from `bindings.ts`, which the UI's tests mock whole: each
/// screen that shows or reads a key would find no table there.
#[test]
fn key_table_bindings_are_up_to_date() {
    let table =
        serde_json::to_string_pretty(tsugiki_core::keys::TABLE).expect("serialize the key table");
    let shifted: serde_json::Map<String, serde_json::Value> =
        tsugiki_core::keys::SHIFTED_ON_US_KEYBOARD
            .iter()
            .map(|(key, shifted)| (key.to_string(), shifted.to_string().into()))
            .collect();
    let shifted = serde_json::to_string_pretty(&shifted).expect("serialize the shifted keys");
    let fresh = format!(
        "// Generated from tsugiki-core's key table by `npm run bindings`. Do not edit.\n\n\
         /** Each action the user can bind, with its title, default key and where it works. */\n\
         export const KEYMAP = {table} as const;\n\n\
         /** What Shift makes each key type on a US keyboard, by what the key types alone. */\n\
         export const SHIFTED: Readonly<Record<string, string>> = {shifted};\n"
    );
    compare_or_update("ui/src/lib/data/keymap.ts", &fresh);
}
