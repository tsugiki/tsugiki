//! A language server that answers lookups in one language, ty or rust-analyzer. One reads a Lookup checkout,
//! with the PR's changed files in its language open on it, and gives the open files the text of the side a
//! lookup is from.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use serde_json::{Value, json};
use tsugiki_lsp::{
    Definitions, Document, Event, Init, Language, Launch, Location, Pending, Position, Server,
    Status,
};

use super::{DiffEnd, Hit, Lookup, PATIENCE, Place, ServerName, failed, plain, slashed, uri};
use crate::diff;
use crate::error::Error;
use crate::mirror::{LookupAt, LookupCheckout};

/// How Tsugiki starts one kind of server, and what it tells it.
pub(super) struct Setup {
    pub(super) args: Vec<OsString>,
    /// Variables the server's process gets, which the processes it starts inherit.
    pub(super) env: Vec<(OsString, OsString)>,
    /// The server's `initializationOptions`.
    pub(super) options: Value,
    /// What the server reads from `workspace/configuration`, by section.
    pub(super) settings: Value,
    /// The language of the files it opens.
    pub(super) language: Language,
    pub(super) ready: Ready,
}

/// When a server can answer a lookup.
pub(super) enum Ready {
    /// Once it has answered `initialize`.
    Initialized,
    /// Once it says it is quiescent, through `experimental/serverStatus`: rust-analyzer finds nothing in a
    /// file until it has loaded the workspace.
    Quiescent,
}

impl Ready {
    /// What the client tells the server it can do, for the server to say when it is ready.
    fn experimental(&self) -> Value {
        match self {
            Self::Initialized => json!({}),
            Self::Quiescent => json!({"serverStatusNotification": true}),
        }
    }
}

/// One language server, which reads a Lookup checkout, with the PR's changed files in its language open on
/// it.
pub(super) struct LanguageServer {
    server: Server,
    /// What the server says unasked, which nothing reads yet. It is emptied on each lookup.
    events: Receiver<Event>,
    folder: PathBuf,
    at: LookupAt,
    open: Vec<Open>,
    /// Which side's text the open files have.
    showing: DiffEnd,
}

/// A file the PR changes, open on the server, with its text at the head and at the base. A file the PR adds
/// has no text at the base.
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

    /// The text the server reads while the files show `side`.
    fn text(&self, side: DiffEnd) -> &Arc<str> {
        match (side, &self.base) {
            (DiffEnd::Base, Some(base)) => base,
            (DiffEnd::Base, None) | (DiffEnd::Head, _) => &self.head,
        }
    }
}

/// What turning the server's answer to a lookup into hits needs: a copy of the server's checkout and open
/// files as the lookup found them, so a thread of the lookup's own reads the answer while the server goes
/// on.
pub(super) struct Hits {
    folder: PathBuf,
    at: LookupAt,
    /// Each open file's path, the commit a hit in it is at, and the text the server read.
    open: Vec<(String, DiffEnd, Arc<str>)>,
}

impl Hits {
    /// The places the server names in `definitions`, in its order.
    pub(super) fn of(&self, definitions: Definitions) -> Vec<Hit> {
        (definitions.into_locations().iter())
            .map(|location| self.hit(location))
            .collect()
    }

    /// `location` as a hit, with the line's text as the server read it.
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

impl LanguageServer {
    /// Starts server `name` at `program` on `checkout`, and opens each of the files at `changed` the head
    /// holds with its text there.
    pub(super) fn start(
        name: ServerName,
        program: &Path,
        checkout: LookupCheckout,
        changed: &[String],
    ) -> Result<Self, Error> {
        let LookupCheckout { folder, at } = checkout;
        // The path the server's answers name the checkout by.
        let folder = plain(&folder);
        let root = uri(&folder)?;
        let folder = root.to_path().unwrap_or(folder);
        let setup = match name {
            ServerName::Ty => super::ty::setup(&folder),
            ServerName::RustAnalyzer => super::rust_analyzer::setup(program),
        };
        let launch = Launch {
            program: program.to_owned(),
            args: setup.args,
            folder: folder.clone(),
            env: setup.env,
        };
        let named = name.as_str();
        let (starting, events) = Server::start(&launch).map_err(|cause| {
            let program = program.display();
            failed(&format!(
                "Tsugiki couldn't start {named} at {program}: {cause}"
            ))
        })?;
        let init = Init {
            root,
            options: setup.options,
            settings: setup.settings,
            experimental: setup.ready.experimental(),
        };
        let (server, _info) = (starting.initialize(init, PATIENCE))
            .map_err(|cause| failed(&format!("{named} didn't start: {cause}")))?;
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
            let document = server.open(uri(&folder.join(path))?, setup.language, &head);
            open.push(Open {
                path: path.clone(),
                document,
                head: head.into(),
                base: base.map(Arc::from),
            });
        }
        if let Ready::Quiescent = setup.ready {
            quiescent(&events);
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

    /// The PR whose checkout this server reads.
    pub(super) fn at(&self) -> &LookupAt {
        &self.at
    }

    /// Asks the server to stop. Its process ends once it agrees, which [`LanguageServer::wait_for_exit`]
    /// waits for, and dropping the server then lets it end on its own.
    pub(super) fn shutdown(&self) -> Pending<()> {
        self.server.shutdown()
    }

    /// Sends the server the request for `lookup`, with the open files at the lookup's side, and returns at
    /// once with what turns its answer into hits.
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

    /// Waits until the server's process has ended, or until `until`.
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

/// Waits until the server says it is quiescent, or has ended, for at most [`PATIENCE`]. A server that is
/// still loading then answers what it can, and one that has ended fails the lookup.
fn quiescent(events: &Receiver<Event>) {
    let until = Instant::now() + PATIENCE;
    while let Ok(event) = events.recv_timeout(until.saturating_duration_since(Instant::now())) {
        let ready = matches!(
            event,
            Event::Status(Status {
                quiescent: true,
                ..
            }) | Event::Exited(_)
        );
        if ready {
            return;
        }
    }
}
