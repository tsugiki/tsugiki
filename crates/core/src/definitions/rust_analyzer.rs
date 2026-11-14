//! How Tsugiki starts rust-analyzer, the language server that answers lookups in Rust, and what it tells it.
//!
//! A lookup never runs the Repository's code (ADR 0020). rust-analyzer runs no build script, builds no proc
//! macro and runs no `cargo check`, and cargo works offline, so it downloads nothing and resolves the
//! dependencies its cache holds. rust-analyzer and what it starts run with the cargo and rustc of the
//! toolchain rust-analyzer is in, whatever the checkout's `rust-toolchain.toml` or `.cargo/config.toml`
//! names, and rustup installs nothing. A rust-analyzer outside a toolchain doesn't start, as the checkout
//! could then pick the programs it runs.

use std::env::consts::EXE_SUFFIX;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tsugiki_lsp::Language;

use super::failed;
use super::server::{Ready, Setup};
use crate::error::Error;

#[cfg(test)]
mod tests;

/// rust-analyzer's setup, for the rust-analyzer at `program`, which has to be inside a toolchain.
pub(super) fn setup(program: &Path) -> Result<Setup, Error> {
    let sysroot = sysroot(program).ok_or_else(|| {
        let program = program.display();
        failed(&format!(
            "{program} isn't inside a Rust toolchain. Pick the rust-analyzer that `rustup which rust-analyzer` names."
        ))
    })?;
    let config = config(&sysroot);
    Ok(Setup {
        args: Vec::new(),
        env: env(&sysroot),
        options: config.clone(),
        settings: json!({"rust-analyzer": config}),
        language: Language::Rust,
        ready: Ready::Quiescent,
    })
}

/// The toolchain `program` is in, as `rustup which rust-analyzer` names it: the folder over its `bin`,
/// which holds the standard library in `lib/rustlib`.
fn sysroot(program: &Path) -> Option<PathBuf> {
    let sysroot = program.parent()?.parent()?;
    let holds = sysroot.join("lib").join("rustlib").is_dir();
    holds.then(|| sysroot.to_owned())
}

/// What rust-analyzer reads from its `initializationOptions`, and again from `workspace/configuration`.
fn config(sysroot: &Path) -> Value {
    json!({
        "cargo": {
            "buildScripts": {"enable": false},
            // Offline, cargo resolves dependencies from what its cache holds. With `noDeps` rust-analyzer
            // would leave the standard library out of each crate's dependencies too.
            "noDeps": false,
            "sysroot": sysroot.display().to_string(),
        },
        "procMacro": {"enable": false, "attributes": {"enable": false}},
        "checkOnSave": false,
        "diagnostics": {"enable": false},
    })
}

/// The variables rust-analyzer runs with, which cargo, rustc and rustup's proxies read in each process it
/// starts. A variable beats what a file in the checkout says.
fn env(sysroot: &Path) -> Vec<(OsString, OsString)> {
    let bin = sysroot.join("bin");
    let mut env: Vec<(OsString, OsString)> = vec![
        ("RUSTUP_TOOLCHAIN".into(), sysroot.into()),
        (
            "CARGO".into(),
            bin.join(format!("cargo{EXE_SUFFIX}")).into(),
        ),
        (
            "RUSTC".into(),
            bin.join(format!("rustc{EXE_SUFFIX}")).into(),
        ),
    ];
    env.extend(
        [
            // rustup installs no toolchain a `rust-toolchain.toml` names.
            ("RUSTUP_AUTO_INSTALL", "0"),
            ("CARGO_NET_OFFLINE", "true"),
            // An empty wrapper is none, whatever `build.rustc-wrapper` says.
            ("RUSTC_WRAPPER", ""),
            ("RUSTC_WORKSPACE_WRAPPER", ""),
        ]
        .map(|(name, value)| (name.into(), value.into())),
    );
    env
}
