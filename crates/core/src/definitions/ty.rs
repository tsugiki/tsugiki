//! ty, the language server that answers lookups in Python. One reads a Lookup checkout, with the PR's changed
//! Python files open on it, and gives the open files the text of the side a lookup is from.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use serde_json::{Value, json};
use tsugiki_lsp::{
    Definitions, Document, Event, Init, Language, Launch, Location, Pending, Position, Server,
};

use super::{DiffEnd, Hit, Lookup, PATIENCE, Place, failed, plain, slashed, uri};
use crate::diff;
use crate::error::Error;
use crate::mirror::{LookupAt, LookupCheckout};

#[cfg(test)]
mod tests;

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
    head: Arc<str>,
    base: Option<Arc<str>>,
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
    fn text(&self, side: DiffEnd) -> &Arc<str> {
        match (side, &self.base) {
            (DiffEnd::Base, Some(base)) => base,
            (DiffEnd::Base, None) | (DiffEnd::Head, _) => &self.head,
        }
    }
}

/// What turning ty's answer to a lookup into hits needs: a copy of the ty's checkout and open files as the
/// lookup found them, so a thread of the lookup's own reads ty's answer while ty goes on.
pub(super) struct Hits {
    folder: PathBuf,
    at: LookupAt,
    /// Each open file's path, the commit a hit in it is at, and the text ty read.
    open: Vec<(String, DiffEnd, Arc<str>)>,
}

impl Hits {
    /// The places ty names in `definitions`, in its order.
    pub(super) fn of(&self, definitions: Definitions) -> Vec<Hit> {
        (definitions.into_locations().iter())
            .map(|location| self.hit(location))
            .collect()
    }

    /// `location` as a hit, with the line's text as ty read it.
    fn hit(&self, location: &Location) -> Hit {
        let start = location.range.start;
        let path = location.uri.to_path().unwrap_or_default();
        let (place, text) = match path.strip_prefix(&self.folder) {
            Ok(relative) => {
                let relative = slashed(relative);
                let open = self.open.iter().find(|(path, ..)| *path == relative);
                let text = open.map(|(.., text)| text.to_string());
                let side = open.map_or(DiffEnd::Base, |(_, side, _)| *side);
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
                head: head.into(),
                base: base.map(Arc::from),
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

    /// Sends ty the request for `lookup`, with the open files at the lookup's side, and returns at once with
    /// what turns ty's answer into hits.
    pub(super) fn ask(&mut self, lookup: &Lookup) -> Result<(Pending<Definitions>, Hits), Error> {
        let _unread = self.events.try_iter().count();
        let document = uri(&self.folder.join(&lookup.path))?;
        self.show(lookup.side);
        let at = Position {
            line: lookup.line.saturating_sub(1),
            character: lookup.column,
        };
        let pending = self.server.definition(&document, at);
        let open = (self.open.iter())
            .map(|open| {
                let text = Arc::clone(open.text(self.showing));
                (open.path.clone(), open.side(lookup.side), text)
            })
            .collect();
        let hits = Hits {
            folder: self.folder.clone(),
            at: self.at.clone(),
            open,
        };
        Ok((pending, hits))
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
                let text = Arc::clone(open.text(side));
                open.document.change(&text);
            }
        }
        self.showing = side;
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
