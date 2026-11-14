//! How Tsugiki starts clangd, the language server that answers lookups in C, and what it tells it.
//!
//! In a Repository with CPython's layout, Tsugiki writes clangd a `compile_commands.json` beside the Lookup
//! checkout, in `repos\<owner>__<name>\servers\clangd\`, where clangd keeps its index too. Each core C file
//! of the checkout gets the same flags, the ones CPython's Windows build gives `pythoncore`, so nothing is
//! configured or built. Elsewhere clangd uses its own defaults.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tsugiki_lsp::Language;

use super::server::{Ready, Setup};
use super::{CPYTHON_MARK, failed};
use crate::error::Error;
use crate::folders::servers_beside;

#[cfg(test)]
mod tests;

/// The folders of CPython's core C files.
const CORE: [&str; 5] = ["Objects", "Python", "Parser", "Modules", "PC"];

/// The core files and folders the database leaves out: vendored libraries, and the source the interpreter's
/// cases are generated from, which isn't C. mimalloc's files build only as part of `Objects/obmalloc.c`,
/// which includes them.
const SKIPPED: [&str; 5] = [
    "Modules/_decimal/libmpdec",
    "Modules/expat",
    "Modules/_hacl",
    "Objects/mimalloc",
    "Python/bytecodes.c",
];

/// The flags each file gets, after the compiler's name.
const FLAGS: [&str; 15] = [
    "-xc",
    "-std=c11",
    "--target=x86_64-pc-windows-msvc",
    "-IInclude",
    "-IInclude/internal",
    "-IInclude/internal/mimalloc",
    "-IPC",
    "-DWIN32",
    "-D_WIN32",
    "-D_WIN64",
    "-D_M_X64",
    "-DPy_BUILD_CORE",
    "-DPy_BUILD_CORE_BUILTIN",
    "-DPy_ENABLE_SHARED",
    "-c",
];

/// clangd's setup for the checkout at `folder`, whose compile database it writes first in a Repository with
/// CPython's layout.
pub(super) fn setup(folder: &Path) -> Result<Setup, Error> {
    let mut args: Vec<OsString> = [
        "--background-index",
        "--background-index-priority=low",
        "-j=4",
    ]
    .map(OsString::from)
    .into();
    if folder.join(CPYTHON_MARK).is_file() {
        let database = database_folder(folder);
        write_database(folder, &database)?;
        let mut arg = OsString::from("--compile-commands-dir=");
        arg.push(&database);
        args.push(arg);
    }
    Ok(Setup {
        args,
        env: Vec::new(),
        options: json!({}),
        settings: json!({}),
        language: Language::C,
        ready: Ready::Initialized,
    })
}

/// Where clangd's compile database and index go for the checkout at `folder`: `servers\clangd\` beside it.
fn database_folder(folder: &Path) -> PathBuf {
    servers_beside(folder).join("clangd")
}

/// Writes `compile_commands.json` into `database` for the core C files of the checkout at `folder`, which
/// holds the PR's added files too. JSON holds only text, so a checkout whose path isn't Unicode gets none.
fn write_database(folder: &Path, database: &Path) -> Result<(), Error> {
    let Some(directory) = folder.to_str() else {
        let folder = folder.display();
        return Err(failed(&format!(
            "Tsugiki can't write clangd's compile database for {folder}, as its path isn't Unicode"
        )));
    };
    let entries: Vec<Value> = (core_files(folder).iter())
        .map(|file| {
            let arguments: Vec<&str> = ["clang"]
                .into_iter()
                .chain(FLAGS)
                .chain([file.as_str()])
                .collect();
            let path = folder.join(file).to_string_lossy().into_owned();
            json!({"directory": directory, "file": path, "arguments": arguments})
        })
        .collect();
    // A `Value` always turns into JSON.
    let text = serde_json::to_vec_pretty(&entries).unwrap_or_default();
    fs::create_dir_all(database)
        .and_then(|()| fs::write(database.join("compile_commands.json"), text))
        .map_err(|cause| {
            let database = database.display();
            failed(&format!(
                "Tsugiki couldn't write clangd's compile database in {database}: {cause}"
            ))
        })
}

/// The paths of the checkout's core C files, with `/` between their parts, in order.
fn core_files(folder: &Path) -> Vec<String> {
    let mut files = Vec::new();
    for top in CORE {
        gather(folder, top, &mut files);
    }
    files.sort();
    files
}

/// Adds the C files under `relative` in the checkout at `folder` to `files`, past those it skips. It follows
/// no link, so a link to a folder above can't loop.
fn gather(folder: &Path, relative: &str, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(folder.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let path = format!("{relative}/{}", entry.file_name().to_string_lossy());
        if SKIPPED.contains(&path.as_str()) {
            continue;
        }
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            gather(folder, &path, files);
        } else if path.ends_with(".c") {
            files.push(path);
        }
    }
}
