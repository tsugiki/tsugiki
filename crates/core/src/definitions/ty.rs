//! How Tsugiki starts ty, the language server that answers lookups in Python, and what it tells it.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tsugiki_lsp::{Fallback, Language};

use super::server::{Ready, Setup, Unopened};

#[cfg(test)]
mod tests;

/// ty's setup for the checkout at `folder`, which ty reads its settings for.
pub(super) fn setup(folder: &Path) -> Setup {
    let (settings, fallback) = settings(folder);
    Setup {
        args: vec![OsString::from("server")],
        env: Vec::new(),
        options: json!({}),
        settings,
        fallback,
        language: Language::Python,
        ready: Ready::Initialized,
        unopened: Unopened::FromDisk,
        index: None,
        troubles: None,
    }
}

/// The settings ty reads its configuration from: in a Repository with CPython's layout, the standard library
/// in `Lib` and the version `Include/patchlevel.h` names, as CPython has no `pyproject.toml` that says so.
/// Elsewhere none, and ty uses its own defaults and the Repository's files.
///
/// ty drops all of its configuration over a version it doesn't know, as ty 0.0.103 does over 3.16, and
/// shows serde's error for it: "unknown variant `3.16`". The fallback then sends `Lib` alone.
fn settings(folder: &Path) -> (Value, Option<Fallback>) {
    let Ok(header) = fs::read_to_string(folder.join(super::CPYTHON_MARK)) else {
        return (json!({}), None);
    };
    let configured =
        |environment: Value| json!({"ty": {"configuration": {"environment": environment}}});
    let lib = configured(json!({"root": ["Lib"]}));
    let Some(version) = python_version(&header) else {
        return (lib, None);
    };
    let fallback = Fallback {
        refusal: format!("unknown variant `{version}`"),
        settings: lib,
    };
    let settings = configured(json!({"root": ["Lib"], "python-version": version}));
    (settings, Some(fallback))
}

/// The `3.15` in a `patchlevel.h` that defines `PY_MAJOR_VERSION` 3 and `PY_MINOR_VERSION` 15.
fn python_version(header: &str) -> Option<String> {
    let defined = |name: &str| {
        header.lines().find_map(|line| {
            let value = line
                .strip_prefix("#define")?
                .trim_start()
                .strip_prefix(name)?;
            value.trim().parse::<u32>().ok()
        })
    };
    Some(format!(
        "{}.{}",
        defined("PY_MAJOR_VERSION")?,
        defined("PY_MINOR_VERSION")?
    ))
}
