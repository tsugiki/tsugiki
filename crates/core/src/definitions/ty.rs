//! How Tsugiki starts ty, the language server that answers lookups in Python, and what it tells it.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use tsugiki_lsp::Language;

use super::server::{Ready, Setup, Unopened};

#[cfg(test)]
mod tests;

/// ty's setup for the checkout at `folder`, which ty reads its settings for.
pub(super) fn setup(folder: &Path) -> Setup {
    Setup {
        args: vec![OsString::from("server")],
        env: Vec::new(),
        options: json!({}),
        settings: settings(folder),
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
/// The checkout is ty's Python there too, or ty takes the `python` on PATH, and Go to definition maps a
/// standard library stub to that Python's source. On Windows ty finds the checkout's stdlib in `Lib`. On
/// Unix ty looks in `lib/pythonX.Y`, which the checkout lacks.
fn settings(folder: &Path) -> Value {
    let Ok(header) = fs::read_to_string(folder.join(super::CPYTHON_MARK)) else {
        return json!({});
    };
    let python = folder.display().to_string();
    let environment = match python_version(&header) {
        Some(version) => json!({"root": ["Lib"], "python": python, "python-version": version}),
        None => json!({"root": ["Lib"], "python": python}),
    };
    json!({"ty": {"configuration": {"environment": environment}}})
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
