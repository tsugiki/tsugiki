//! ty, the language server that answers lookups in Python. One reads a Lookup checkout, with the PR's changed
//! Python files open on it, and gives the open files the text of the side a lookup is from.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tsugiki_lsp::{Document, Event, Init, Language, Launch, Location, Pending, Position, Server};

use super::{Answer, DiffEnd, Hit, Lookup, PATIENCE, Place, failed, plain, slashed, uri};
use crate::diff;
use crate::error::Error;
use crate::mirror::{LookupAt, LookupCheckout};

#[cfg(test)]
mod tests;

/// Why ty gave no answer to a lookup.
pub(super) enum Failure {
    /// It is still working on it, as on its first lookup in a big Repository, and goes on running.
    Slow(Error),
    /// It answered with an error, or ended, and starts afresh on the next lookup.
    Broken(Error),
}

/// One ty, which reads a Lookup checkout, with the PR's changed Python files open on it.
pub(super) struct Ty {
    server: Server,
    /// What ty says unasked, which nothing reads yet. It is emptied on each lookup.
    events: Receiver<Event>,
    folder: PathBuf,
    at: LookupAt,
    open: Vec<Open>,
    /// Which side's text the open files have.
    showing: DiffEnd,
}

/// A file the PR changes, open on ty, with its text at the head and at the base. A file the PR adds has no
/// text at the base.
struct Open {
    path: String,
    document: Document,
    head: String,
    base: Option<String>,
}

impl Open {
    /// The commit a hit in this file is at, asked at `side`: the head for a file the base doesn't hold at
    /// this path, as one the PR adds or renames.
    fn side(&self, asked: DiffEnd) -> DiffEnd {
        if self.base.is_some() {
            asked
        } else {
            DiffEnd::Head
        }
    }

    /// The text ty reads while the files show `side`.
    fn text(&self, side: DiffEnd) -> &str {
        match (side, &self.base) {
            (DiffEnd::Base, Some(base)) => base,
            (DiffEnd::Base, None) | (DiffEnd::Head, _) => &self.head,
        }
    }
}

impl Ty {
    /// Starts ty at `program` on `checkout`, and opens each of the files at `changed` the head holds with its
    /// text there.
    pub(super) fn start(
        program: &Path,
        checkout: LookupCheckout,
        changed: &[String],
    ) -> Result<Self, Error> {
        let LookupCheckout { folder, at } = checkout;
        // The path the server's answers name the checkout by.
        let folder = plain(&folder);
        let root = uri(&folder)?;
        let folder = root.to_path().unwrap_or(folder);
        let launch = Launch {
            program: program.to_owned(),
            args: vec![OsString::from("server")],
            folder: folder.clone(),
        };
        let (starting, events) = Server::start(&launch).map_err(|cause| {
            let program = program.display();
            failed(&format!("Tsugiki couldn't start ty at {program}: {cause}"))
        })?;
        let init = Init {
            root,
            options: json!({}),
            settings: settings(&folder),
        };
        let (server, _info) = (starting.initialize(init, PATIENCE))
            .map_err(|cause| failed(&format!("ty didn't start: {cause}")))?;
        let names: Vec<String> = (changed.iter())
            .flat_map(|path| {
                [&at.head, &at.base].map(|commit| format!("{}:{path}", commit.as_str()))
            })
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut texts = diff::blobs(&folder, &names).into_iter();
        let mut open = Vec::new();
        for path in changed {
            // A file the PR deletes has no text at the head, and the checkout holds its base.
            let (Some(Some(head)), base) = (texts.next(), texts.next().flatten()) else {
                continue;
            };
            let document = server.open(uri(&folder.join(path))?, Language::Python, &head);
            open.push(Open {
                path: path.clone(),
                document,
                head,
                base,
            });
        }
        Ok(Self {
            server,
            events,
            folder,
            at,
            open,
            showing: DiffEnd::Head,
        })
    }

    /// The PR whose checkout this ty reads.
    pub(super) fn at(&self) -> &LookupAt {
        &self.at
    }

    /// Asks ty to stop. Its process ends once it agrees, which [`Ty::wait_for_exit`] waits for, and dropping
    /// the server then lets it end on its own.
    pub(super) fn shutdown(&self) -> Pending<()> {
        self.server.shutdown()
    }

    /// Asks ty, and waits up to `patience` for its answer. A lookup ty hasn't answered by then is cancelled.
    pub(super) fn look_up(
        &mut self,
        lookup: &Lookup,
        patience: Duration,
    ) -> Result<Answer, Failure> {
        let _unread = self.events.try_iter().count();
        self.show(lookup.side);
        let at = Position {
            line: lookup.line.saturating_sub(1),
            character: lookup.column,
        };
        let document = uri(&self.folder.join(&lookup.path)).map_err(Failure::Broken)?;
        let pending = self.server.definition(&document, at);
        let Some(answer) = pending.wait_timeout(patience) else {
            pending.cancel();
            let slow = "ty is still reading the PR's files. Ask again in a moment.";
            return Err(Failure::Slow(failed(slow)));
        };
        let definitions = answer
            .map_err(|cause| Failure::Broken(failed(&format!("ty couldn't answer: {cause}"))))?;
        let hits = (definitions.into_locations().iter())
            .map(|location| self.hit(lookup.side, location))
            .collect();
        Ok(Answer::Found { hits })
    }

    /// Waits until ty's process has ended, or until `until`.
    pub(super) fn wait_for_exit(&self, until: Instant) {
        while let Ok(event) =
            (self.events).recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            if matches!(event, Event::Exited(_)) {
                return;
            }
        }
    }

    /// Gives the open files their text at `side`, if they don't have it.
    fn show(&mut self, side: DiffEnd) {
        if self.showing == side {
            return;
        }
        for open in &mut self.open {
            if open.base.is_some() {
                let text = open.text(side).to_owned();
                open.document.change(&text);
            }
        }
        self.showing = side;
    }

    /// `location` as a hit of a lookup at `side`, with the line's text as ty read it.
    fn hit(&self, side: DiffEnd, location: &Location) -> Hit {
        let start = location.range.start;
        let path = location.uri.to_path().unwrap_or_default();
        let (place, text) = match path.strip_prefix(&self.folder) {
            Ok(relative) => {
                let relative = slashed(relative);
                let open = self.open.iter().find(|open| open.path == relative);
                let text = open.map(|open| open.text(self.showing).to_owned());
                let side = open.map_or(DiffEnd::Base, |open| open.side(side));
                let commit = match side {
                    DiffEnd::Base => self.at.base.clone(),
                    DiffEnd::Head => self.at.head.clone(),
                };
                let place = Place::Repository {
                    path: relative,
                    side,
                    commit,
                };
                (place, text)
            }
            Err(_outside) => {
                let place = Place::Outside {
                    path: path.display().to_string(),
                };
                (place, None)
            }
        };
        let text = text.unwrap_or_else(|| fs::read_to_string(&path).unwrap_or_default());
        let line = usize::try_from(start.line).unwrap_or(usize::MAX);
        Hit {
            place,
            line: start.line.saturating_add(1),
            column: start.character,
            text: text.lines().nth(line).unwrap_or_default().to_owned(),
        }
    }
}

/// The settings ty reads its configuration from: in a Repository with CPython's layout, the standard library
/// in `Lib` and the version `Include/patchlevel.h` names, as CPython has no `pyproject.toml` that says so.
/// Elsewhere none, and ty uses its own defaults and the Repository's files.
fn settings(folder: &Path) -> Value {
    let Ok(header) = fs::read_to_string(folder.join("Include/patchlevel.h")) else {
        return json!({});
    };
    let environment = match python_version(&header) {
        Some(version) => json!({"root": ["Lib"], "python-version": version}),
        None => json!({"root": ["Lib"]}),
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
