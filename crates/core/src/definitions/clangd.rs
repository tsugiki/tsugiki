//! How Tsugiki starts clangd, the language server that answers lookups in C, and what it tells it.
//!
//! In a Repository with CPython's layout, Tsugiki writes clangd a `compile_commands.json` beside the Lookup
//! checkout, in `repos\<owner>__<name>\servers\clangd\`, where clangd keeps its index too. Each core C file
//! of the checkout gets the same flags, the ones CPython's Windows build gives `pythoncore`, so nothing is
//! configured or built. Elsewhere clangd uses its own defaults.
//!
//! clangd fails in ways that look like wrong answers, so Tsugiki names each (#684): its built-in headers
//! missing, the system's headers missing, and an index that never starts.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs};

use serde_json::{Value, json};
use tsugiki_lsp::{Diagnostics, Language, Progress, ProgressToken, Uri, WorkDone};

use super::server::{Indexing, Ready, Setup};
use super::{CPYTHON_MARK, IndexedFiles, ServerProblem, failed};
use crate::error::Error;
use crate::folders::servers_beside;
use crate::github::{CommitSha, live};

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

/// The token clangd reports its background index's progress on.
const INDEX_TOKEN: &str = "backgroundIndexProgress";

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

/// The longest clangd gets from the first file opened to report its index, before Tsugiki says it isn't
/// indexing. It reports the index once it has read its compile database, which it does at once.
const INDEX_PATIENCE: Duration = Duration::from_secs(60);

/// What Tsugiki watches clangd's events for, to say why it can't answer (#684).
pub(super) struct Troubles {
    /// The clangd that runs, beside which its built-in headers are.
    pub(super) program: PathBuf,
    /// The longest from the first file opened to the first report of its index.
    pub(super) index_within: Duration,
}

/// clangd's setup for the checkout at `folder`, whose compile database it writes first in a Repository with
/// CPython's layout, with clangd at `program`.
pub(super) fn setup(folder: &Path, program: &Path) -> Result<Setup, Error> {
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
        index: Some(indexing),
        troubles: Some(Troubles {
            program: program.to_owned(),
            index_within: INDEX_PATIENCE,
        }),
    })
}

/// Whether the clangd at `program` has its built-in headers where it looks for them, past links: a
/// `lib\clang\<N>\include\stddef.h` in the folder above its own. A bare name is found through `PATH`, as
/// its start finds it. It reads a folder and looks at a file in each of its folders.
pub(super) fn has_built_ins(program: &Path) -> bool {
    let program = on_path(program, env::var_os("PATH"));
    let program = fs::canonicalize(&program).unwrap_or(program);
    let clang = (program.ancestors().nth(2)).map(|root| root.join("lib").join("clang"));
    let versions = clang.and_then(|clang| fs::read_dir(clang).ok());
    (versions.into_iter().flatten().flatten())
        .any(|version| version.path().join("include").join("stddef.h").is_file())
}

/// What the system adds to a bare program's name as it looks for it on `PATH`, in order. A constant, so that
/// no line of `on_path` runs on one system only, and coverage holds on each.
const SUFFIXES: &[&str] = if cfg!(windows) { &[".exe", ""] } else { &[""] };

/// Where the program at `program` is: a bare name in the first folder of `path` that holds it, with `.exe`
/// first on Windows, as the system starts it. Another path, or a name no folder holds, stays as it is.
fn on_path(program: &Path, path: Option<OsString>) -> PathBuf {
    let bare = program.components().count() == 1;
    let names: Vec<PathBuf> = (SUFFIXES.iter())
        .map(|suffix| {
            let mut name = program.as_os_str().to_owned();
            name.push(suffix);
            PathBuf::from(name)
        })
        .collect();
    let folders = path.filter(|_| bare).unwrap_or_default();
    (env::split_paths(&folders))
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| program.to_owned())
}

/// What clangd's diagnostics on the open files say of the headers it can't find (#684). A header it can't
/// find stops its parse of the file, so it reports the first one only. It reads the disk and git, so a
/// thread of its own reads the diagnostics, and it keeps what it read for the next ones.
pub(super) struct Headers {
    program: PathBuf,
    /// The checkout's folder, and the commit whose files are the Repository's: the PR's head.
    folder: PathBuf,
    head: CommitSha,
    /// Each open file's URI, and its texts at the head and at the base, which say how it includes a header.
    open: Vec<(Uri, Vec<Arc<str>>)>,
    /// Whether clangd's built-in headers are beside it, once a header is missing.
    built_ins: Option<bool>,
    /// The text of each file a note has named, which says how it includes a header.
    included: HashMap<String, String>,
    /// The paths of the Repository's files, once git has listed them.
    own: Option<Vec<String>>,
}

impl Headers {
    /// The reader of the diagnostics of clangd at `program`, on the checkout at `folder` of a PR whose head
    /// is `head`, with the files `open` open.
    pub(super) fn new(
        program: PathBuf,
        (folder, head): (PathBuf, CommitSha),
        open: Vec<(Uri, Vec<Arc<str>>)>,
    ) -> Self {
        Self {
            program,
            folder,
            head,
            open,
            built_ins: None,
            included: HashMap::new(),
            own: None,
        }
    }

    /// The problem `diagnostics` say clangd has, if they say it can't find a header of the system's: one
    /// included in angle brackets that isn't the Repository's, or `stddef.h`. That is its built-in headers
    /// missing, when they aren't beside it, and else the system's. When git can't list the Repository's
    /// files, a header in angle brackets says nothing.
    pub(super) fn problem(&mut self, diagnostics: &Diagnostics) -> Option<ServerProblem> {
        let missing = (diagnostics.diagnostics.iter())
            .find_map(|diagnostic| Missing::read(&diagnostic.message))?;
        let bracketed = self.bracketed(&missing, &diagnostics.uri);
        let system = missing.header == "stddef.h"
            || (bracketed && self.holds(missing.header) == Some(false));
        let built_ins = *(self.built_ins).get_or_insert_with(|| has_built_ins(&self.program));
        system.then(|| {
            if built_ins {
                ServerProblem::SystemHeadersMissing {
                    header: missing.header.to_owned(),
                }
            } else {
                ServerProblem::BuiltInHeadersMissing
            }
        })
    }

    /// Whether the file that includes the `missing` header includes it in angle brackets: the open file at
    /// `uri`, at either side, or the file a note names.
    fn bracketed(&mut self, missing: &Missing<'_>, uri: &Uri) -> bool {
        match missing.includer {
            Includer::Diagnosed => (self.open.iter())
                .filter(|(open, _)| open == uri)
                .flat_map(|(_, texts)| texts)
                .any(|text| includes_bracketed(text, missing.header)),
            Includer::File(path) => {
                let text = (self.included.entry(path.to_owned()))
                    .or_insert_with(|| fs::read_to_string(path).unwrap_or_default());
                includes_bracketed(text, missing.header)
            }
        }
    }

    /// Whether the Repository holds a file at `header`, or at a path that ends in it, as
    /// `Modules/_decimal/libmpdec/mpdecimal.h` ends in `mpdecimal.h`, or nothing if git can't list its
    /// files. A list git failed to give is asked again the next time.
    fn holds(&mut self, header: &str) -> Option<bool> {
        if self.own.is_none() {
            self.own = self.listed();
        }
        let nested = format!("/{header}");
        let own = self.own.as_ref()?;
        Some((own.iter()).any(|path| path == header || path.ends_with(&nested)))
    }

    /// The paths of the files at the PR's head, as git lists them, if it can.
    fn listed(&self) -> Option<Vec<String>> {
        let args = [
            "-C",
            "",
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            "--full-tree",
            "",
        ];
        let mut args = args.map(OsStr::new);
        args[1] = self.folder.as_os_str();
        args[7] = OsStr::new(self.head.as_str());
        let listed = live::read_git(&args).ok()?;
        Some(listed.split('\0').map(str::to_owned).collect())
    }
}

/// A header a diagnostic of clangd's says it can't find, and the file that includes it.
struct Missing<'a> {
    header: &'a str,
    includer: Includer<'a>,
}

/// The file that includes a header clangd can't find.
enum Includer<'a> {
    /// The open file the diagnostic is on.
    Diagnosed,
    /// A file the open file includes, at this path, as the diagnostic's note names it, or nowhere if it
    /// names none.
    File(&'a str),
}

impl<'a> Missing<'a> {
    /// The missing header `message` names: `'stdlib.h' file not found` on the open file's include, or
    /// `In included file: 'stdlib.h' file not found` on the include of a file that includes it, which a
    /// note after a blank line names as `<path>:<line>:<column>:`.
    fn read(message: &'a str) -> Option<Self> {
        let mut parts = message.split("\n\n");
        let said = parts.next().unwrap_or_default();
        let (said, nested) = match said.strip_prefix("In included file: ") {
            Some(said) => (said, true),
            None => (said, false),
        };
        let header = said.strip_prefix('\'')?.strip_suffix("' file not found")?;
        let includer = if nested {
            let note = parts.next().and_then(|note| note.lines().next());
            let path = note.and_then(|note| note.rsplitn(4, ':').nth(3));
            Includer::File(path.unwrap_or_default())
        } else {
            Includer::Diagnosed
        };
        Some(Self { header, includer })
    }
}

/// Whether `text` includes `header` in angle brackets, as `#include <stdlib.h>` does.
fn includes_bracketed(text: &str, header: &str) -> bool {
    text.lines().any(|line| {
        let directive = line.trim_start().strip_prefix('#').map(str::trim_start);
        let named = (directive.and_then(|directive| directive.strip_prefix("include")))
            .and_then(|rest| rest.trim_start().strip_prefix('<'))
            .and_then(|rest| rest.split_once('>'));
        named.is_some_and(|(name, _)| name.trim() == header)
    })
}

/// The progress of clangd's background index, from a `$/progress` on its token, or `None` for other
/// progress. Each report's message counts the files as `done/total`, from the moment the index last had
/// nothing to do.
pub(super) fn indexing(progress: &Progress) -> Option<Indexing> {
    if !matches!(&progress.token, ProgressToken::Text(token) if token == INDEX_TOKEN) {
        return None;
    }
    Some(match &progress.value {
        WorkDone::Begin { message, .. } | WorkDone::Report { message, .. } => {
            Indexing::Building(message.as_deref().and_then(files))
        }
        WorkDone::End { .. } => Indexing::Built,
    })
}

/// The files a report's `done/total` counts, if it reads so.
fn files(message: &str) -> Option<IndexedFiles> {
    let (done, total) = message.split_once('/')?;
    Some(IndexedFiles {
        done: done.trim().parse().ok()?,
        total: total.trim().parse().ok()?,
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
