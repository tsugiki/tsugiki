//! A language server that answers lookups in one language: ty, rust-analyzer or clangd. One reads a Lookup
//! checkout, with the PR's changed files in its language open on it, and gives the open files the text of
//! the side a lookup is from.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tsugiki_lsp::{
    Definitions, Document, Event, Init, Language, Launch, Location, Pending, Position, Progress,
    Server, Starting, Status, Uri,
};

use super::{
    DiffEnd, EXIT_PATIENCE, Hit, IndexedFiles, Lookup, PATIENCE, Place, ServerName, failed, plain,
    slashed, uri,
};
use crate::diff;
use crate::error::Error;
use crate::latest::{self, LatestReader, LatestWriter};
use crate::mirror::{LookupAt, LookupCheckout};

#[cfg(test)]
mod tests;

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
    /// Reads how far the server's index has got from a `$/progress`, for a server whose answers may miss
    /// what its index doesn't hold yet, as clangd's do.
    pub(super) index: Option<fn(&Progress) -> Option<Indexing>>,
}

/// A server's progress with its index, as its `$/progress` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Indexing {
    /// It hasn't reported its index yet. clangd reports one once it has read its compile database, which it
    /// does when the first file opens, and never without one.
    Unreported,
    /// It builds its index, and has indexed so many files of so many, once it has said.
    Building(Option<IndexedFiles>),
    /// It has built its index, or keeps none Tsugiki reads.
    Built,
    /// Its events have ended, as the server has.
    Ended,
}

impl Indexing {
    /// Whether a server that was so far along at `told` has news worth waking the servers' thread for at
    /// `self`, `since` then: a new state, or the first count, or a new count once [`NEWS_EVERY`] has passed.
    /// clangd reports each file it indexes, 452 of them in CPython's core.
    pub(super) fn news_since(self, told: Self, since: Duration) -> bool {
        match (told, self) {
            (Self::Building(Some(_)), Self::Building(Some(_))) => since >= NEWS_EVERY,
            _ => told != self,
        }
    }
}

/// The shortest time between two counts of a server's indexed files that wake the servers' thread.
pub(super) const NEWS_EVERY: Duration = Duration::from_secs(1);

/// The listener to a server's index moving on: the servers' thread, which then reads how far.
pub(super) type IndexMoved = Box<dyn Fn() + Send>;

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
    /// The events the server sends unasked, past the thread that reads how far its index has got. A stop
    /// waits on them, and they are emptied on each lookup.
    events: Receiver<Event>,
    folder: PathBuf,
    at: LookupAt,
    open: Vec<Open>,
    /// Which side's text the open files have.
    showing: DiffEnd,
    /// The moment the server started, or a lookup last asked it or got its answer.
    used: Instant,
    /// Its progress with its index, which a thread that reads its events writes.
    indexing: LatestReader<Indexing>,
}

/// A lookup as one server reads it: the document, the place in it, and the side whose text the open files
/// have. Asking a server it was made for again can't fail.
pub(super) struct Request {
    document: Uri,
    at: Position,
    side: DiffEnd,
}

/// A server asked to stop, whose end the caller may wait for.
pub(super) struct Stopping(Receiver<Event>);

impl Stopping {
    /// Waits until the server's process has ended, or until `until`. The events end once the client has
    /// killed it.
    pub(super) fn wait(self, until: Instant) {
        while let Ok(event) = (self.0).recv_timeout(until.saturating_duration_since(Instant::now()))
        {
            if matches!(event, Event::Exited(_)) {
                return;
            }
        }
    }
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
    /// holds with its text there. `moved` hears each time its index moves on.
    pub(super) fn start(
        name: ServerName,
        program: &Path,
        (checkout, changed): (LookupCheckout, &[String]),
        moved: IndexMoved,
    ) -> Result<Self, Error> {
        let (folder, _root) = located(&checkout.folder)?;
        let setup = match name {
            ServerName::Ty => super::ty::setup(&folder),
            ServerName::RustAnalyzer => super::rust_analyzer::setup(program)?,
            ServerName::Clangd => super::clangd::setup(&folder)?,
        };
        let launch = Launch {
            program: program.to_owned(),
            args: setup.args.clone(),
            folder,
            env: setup.env.clone(),
        };
        let started = Server::start(&launch).map_err(|cause| {
            let (named, program) = (name.as_str(), program.display());
            failed(&format!(
                "Tsugiki couldn't start {named} at {program}: {cause}"
            ))
        })?;
        Self::initialize(name, (setup, started), (checkout, changed), moved)
    }

    /// Initializes server `name` with `setup`, through the client that talks to it and its events, on
    /// `checkout`, and opens each of the files at `changed` the head holds with its text there.
    /// [`LanguageServer::start`] spawns the server, and tests talk to a fake one over in-memory pipes.
    pub(super) fn initialize(
        name: ServerName,
        (setup, (starting, events)): (Setup, (Starting, Receiver<Event>)),
        (checkout, changed): (LookupCheckout, &[String]),
        moved: IndexMoved,
    ) -> Result<Self, Error> {
        let LookupCheckout { folder, at } = checkout;
        let (folder, root) = located(&folder)?;
        let named = name.as_str();
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
        let (writer, indexing) = latest::channel(Indexing::Built);
        let events = match setup.index {
            Some(read) => watch_index(events, read, writer, moved),
            None => events,
        };
        Ok(Self {
            server,
            events,
            folder,
            at,
            open,
            showing: DiffEnd::Head,
            used: Instant::now(),
            indexing,
        })
    }

    /// The server's progress with its index.
    pub(super) fn indexing(&self) -> Indexing {
        self.indexing.get()
    }

    /// The PR whose checkout this server reads.
    pub(super) fn at(&self) -> &LookupAt {
        &self.at
    }

    /// The moment the server started, or a lookup last asked it or got its answer.
    pub(super) fn used(&self) -> Instant {
        self.used
    }

    /// Notes that a lookup got the server's answer now.
    pub(super) fn answered(&mut self) {
        self.used = Instant::now();
    }

    /// Asks the server to stop with `shutdown` and `exit`, and returns at once. One that hasn't agreed within
    /// [`EXIT_PATIENCE`], or hasn't exited [`EXIT_PATIENCE`] after `exit`, is killed.
    pub(super) fn stop(self) -> Stopping {
        self.server.stop(EXIT_PATIENCE);
        Stopping(self.events)
    }

    /// The request for `lookup`, as this server reads it.
    pub(super) fn request(&self, lookup: &Lookup) -> Result<Request, Error> {
        let document = uri(&self.folder.join(&lookup.path))?;
        let at = Position {
            line: lookup.line.saturating_sub(1),
            character: lookup.column,
        };
        Ok(Request {
            document,
            at,
            side: lookup.side,
        })
    }

    /// Sends the server `request`, with the open files at the request's side, and returns at once with what
    /// turns its answer into hits.
    pub(super) fn ask(&mut self, request: &Request) -> (Pending<Definitions>, Hits) {
        self.used = Instant::now();
        let _unread = self.events.try_iter().count();
        self.show(request.side);
        let pending = self.server.definition(&request.document, request.at);
        let open = (self.open.iter())
            .map(|open| {
                let text = Arc::clone(open.text(self.showing));
                (open.path.clone(), open.side(request.side), text)
            })
            .collect();
        let hits = Hits {
            folder: self.folder.clone(),
            at: self.at.clone(),
            open,
        };
        (pending, hits)
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

/// The path the server's answers name the checkout at `folder` by, without its `.` and `..` parts, and its
/// URI.
fn located(folder: &Path) -> Result<(PathBuf, Uri), Error> {
    let folder = plain(folder);
    let root = uri(&folder)?;
    Ok((root.to_path().unwrap_or(folder), root))
}

/// Reads the server's `events` on a thread of their own as they come, writes how far its index has got
/// into `indexing` as `read` finds it, starting from [`Indexing::Unreported`], and tells `moved` when that
/// is news. A count it held back as too soon it tells once [`NEWS_EVERY`] has passed, though the server
/// says nothing more. It hands each event on, through the receiver it gives. Once the server has exited, or
/// its events end, it writes [`Indexing::Ended`] and tells `moved`.
fn watch_index(
    events: Receiver<Event>,
    read: fn(&Progress) -> Option<Indexing>,
    mut indexing: LatestWriter<Indexing>,
    moved: IndexMoved,
) -> Receiver<Event> {
    let (passes, passed) = mpsc::channel();
    indexing.set(&Indexing::Unreported);
    let _reads = thread::spawn(move || {
        let mut told = (Indexing::Unreported, Instant::now());
        let mut latest = Indexing::Unreported;
        loop {
            // A wait too long for a deadline waits for the next event.
            let wait = if latest == told.0 {
                Duration::MAX
            } else {
                NEWS_EVERY.saturating_sub(told.1.elapsed())
            };
            let event = events.recv_timeout(wait);
            if let Err(RecvTimeoutError::Timeout) = event {
                told = (latest, Instant::now());
                moved();
                continue;
            }
            let Ok(event) = event else { break };
            if let Event::Progress(progress) = &event
                && let Some(now) = read(progress)
            {
                indexing.set(&now);
                latest = now;
                if now.news_since(told.0, told.1.elapsed()) {
                    told = (now, Instant::now());
                    moved();
                }
            }
            let exited = matches!(event, Event::Exited(_));
            // The server is gone once nothing reads its events.
            if passes.send(event).is_err() || exited {
                break;
            }
        }
        indexing.set(&Indexing::Ended);
        moved();
    });
    passed
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
