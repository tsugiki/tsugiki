//! The installers read `tauri.conf.json` and the running program reads `Build`, so the two have to agree on
//! what a release is called, or links would start a program that doesn't answer them.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and this helper sits outside any #[test] function"
)]

use std::fs;
use std::path::Path;

use serde_json::Value;
use tsugiki_core::build::Build;

fn config() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_config_has_the_release_identifier() {
    assert_eq!(config()["identifier"], Build::Release.identifier());
}

#[test]
fn the_installers_register_only_the_release_scheme() {
    assert_eq!(
        config()["plugins"]["deep-link"]["desktop"]["schemes"],
        serde_json::json!([Build::Release.scheme()])
    );
}
