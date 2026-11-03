//! The limits ADR 0005 sets on what the app can reach. Changing one should be a visible decision.

#![expect(
    clippy::unwrap_used,
    reason = "a test fails by panicking, and these helpers sit outside any #[test] function"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn json(path: &str) -> Value {
    let text = fs::read_to_string(repo().join(path)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The Rust sources in `dir` and below.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
    sources
}

/// The hosts of the `http://` and `https://` addresses in `text`.
fn hosts(text: &str) -> Vec<String> {
    text.split("://")
        .zip(text.split("://").skip(1))
        .filter(|(before, _)| before.ends_with("http") || before.ends_with("https"))
        .map(|(_, after)| {
            let is_host = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
            after.chars().take_while(|c| is_host(*c)).collect()
        })
        .collect()
}

#[test]
fn rust_names_only_github_as_a_host() {
    let mut found = BTreeSet::new();
    for crate_src in ["crates/core/src", "crates/app/src"] {
        for source in rust_sources(&repo().join(crate_src)) {
            found.extend(hosts(&fs::read_to_string(source).unwrap()));
        }
    }
    assert!(found.contains("api.github.com"), "the scan found {found:?}");
    let allowed = BTreeSet::from(["api.github.com".to_owned(), "github.com".to_owned()]);
    assert!(
        found.is_subset(&allowed),
        "the core connects only to GitHub, not {found:?}"
    );
}

#[test]
fn core_and_app_forbid_unsafe_code() {
    for root in [
        "crates/core/src/lib.rs",
        "crates/app/src/lib.rs",
        "crates/app/src/main.rs",
    ] {
        let source = fs::read_to_string(repo().join(root)).unwrap();
        assert!(source.contains("#![forbid(unsafe_code)]"), "{root}");
    }
}

/// Lines of Rust that use the `unsafe` keyword, in `dir` and below. Comments don't count, nor does this file,
/// which has to spell the keyword to look for it.
fn unsafe_lines(dir: &Path) -> usize {
    rust_sources(dir)
        .iter()
        .filter(|source| source.file_name().is_none_or(|name| name != "security.rs"))
        .map(|source| fs::read_to_string(source).unwrap())
        .map(|text| {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//") && line.contains("unsafe "))
                .count()
        })
        .sum()
}

#[test]
fn webview_keys_has_the_one_unsafe_block_and_no_other_crate_has_any() {
    for entry in fs::read_dir(repo().join("crates")).unwrap() {
        let path = entry.unwrap().path();
        let expected = usize::from(path.ends_with("webview-keys"));
        assert_eq!(unsafe_lines(&path), expected, "{}", path.display());
    }
}

#[test]
fn the_webview_has_the_strict_csp() {
    let config = json("crates/app/tauri.conf.json");
    assert_eq!(
        config["app"]["security"]["csp"],
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data:; connect-src ipc: http://ipc.localhost"
    );
}

#[test]
fn the_webview_has_only_tauris_core_permissions() {
    let capabilities = json("crates/app/capabilities/default.json");
    assert_eq!(
        capabilities["permissions"],
        serde_json::json!(["core:default"])
    );
}

#[test]
fn no_http_shell_or_file_system_plugin_is_installed() {
    let manifests = ["crates/app/Cargo.toml", "package.json", "ui/package.json"];
    for manifest in manifests {
        let text = fs::read_to_string(repo().join(manifest)).unwrap();
        for plugin in ["http", "shell", "fs"] {
            assert!(
                !text.contains(&format!("tauri-plugin-{plugin}")),
                "{manifest}: {plugin}"
            );
            assert!(
                !text.contains(&format!("plugin-{plugin}\"")),
                "{manifest}: {plugin}"
            );
        }
    }
}
