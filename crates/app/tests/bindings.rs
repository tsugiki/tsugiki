//! Fails when the committed TypeScript bindings differ from what the commands generate (ADR 0008).
//! `TSUGIKI_UPDATE_BINDINGS=1` rewrites them instead; `npm run bindings` does that.

use std::path::Path;

use specta_typescript::Typescript;

const HEADER: &str = "// Generated from tsugiki-app's commands by `npm run bindings`. Do not edit.";

#[test]
fn bindings_are_up_to_date() {
    let fresh_path =
        std::env::temp_dir().join(format!("tsugiki-bindings-{}.ts", std::process::id()));
    tsugiki_app::specta_builder()
        .export(Typescript::default().header(HEADER), &fresh_path)
        .expect("export bindings");
    let fresh = std::fs::read_to_string(&fresh_path).expect("read fresh bindings");
    std::fs::remove_file(&fresh_path).ok();

    let committed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/src/lib/data/bindings.ts");
    if std::env::var("TSUGIKI_UPDATE_BINDINGS").as_deref() == Ok("1") {
        std::fs::write(committed, &fresh).expect("write bindings");
        return;
    }
    assert!(
        std::fs::read_to_string(committed).unwrap_or_default() == fresh,
        "the TypeScript bindings are out of date: run `npm run bindings` and commit the result"
    );
}
