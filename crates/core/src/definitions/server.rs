//! A language server that answers lookups in one language: ty, rust-analyzer or clangd. One reads a Lookup
//! checkout, with the PR's changed files in its language open on it, and gives the open files the text of
//! the side a lookup is from. On clangd, which answers only in a file it was sent, a file the PR doesn't
//! change opens once a lookup asks in it, and the last 16 asked about stay open.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tsugiki_lsp::{
    Definitions, Diagnostics, Document, Event, Exit, Fallback, Init, Language, Launch, Location,
    Pending, Position, Progress, RequestError, Server, Starting, Status, Uri,
};

use super::clangd::{Headers, Troubles};
use super::health::version_in;
use super::{
    DiffEnd, EXIT_NEWS, EXIT_PATIENCE, Hit, IndexedFiles, Lookup, NotReady, PATIENCE, Place,
    ServerName, ServerProblem, plain, server_log, slashed, uri,
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
    /// The settings it reads instead if it refuses `settings`.
    pub(super) fallback: Option<Fallback>,
    /// The language of the files it opens.
    pub(super) language: Language,
    pub(super) ready: Ready,
    /// How it reads a file that isn't open on it.
    pub(super) unopened: Unopened,
    /// Reads how far the server's index has got from a `$/progress`, for a server whose answers may miss
    /// what its index doesn't hold yet, as clangd's do.
    pub(super) index: Option<fn(&Progress) -> Option<Indexing>>,
    /// What Tsugiki watches the server's events for, to say why it can't answer, which only clangd has.
    pub(super) troubles: Option<Troubles>,
}

/// How a server reads a file that isn't open on it, such as one the PR doesn't change.
#[derive(Clone, Copy)]
pub(super) enum Unopened {
    /// From disk, as ty and rust-analyzer do.
    FromDisk,
    /// Not at all: clangd builds an AST only for a document it was sent, and refuses a lookup in another
    /// with "trying to get AST for non-added document" (#750). A lookup in such a file opens it first.
    Refused,
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

/// What a server's events tell the servers' thread.
pub(super) enum News {
    /// Its index has moved on, and the thread reads how far. Once the server has ended, it reads that.
    Indexing,
    /// It exited, as its process ended so.
    Exited(Exit),
    /// What it reports, or doesn't, says it runs with this problem (#684).
    Troubled(ServerProblem),
}

/// The listener to a server's news: the servers' thread. The thread that reads clangd's diagnostics tells
/// it too.
pub(super) type OnNews = Arc<dyn Fn(News) + Send + Sync>;

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
    /// The events the server sends unasked, past the thread that reads how far its index has got and
    /// whether it exited. A stop waits on them, and they are emptied on each lookup.
    events: Receiver<Event>,
    /// The version the server named answering `initialize`, if it did.
    version: Option<String>,
    folder: PathBuf,
    at: LookupAt,
    /// The language of the files it opens.
    language: Language,
    open: Vec<Open>,
    /// The files the PR doesn't change that lookups asked in, open with their text in the checkout, the one
    /// asked about longest ago first. None for a server that reads such a file from disk.
    visited: Option<VecDeque<Document>>,
    /// The thread that reads the server's events, which hears when a lookup opens a file.
    watcher: Sender<Heard>,
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

/// The most files the PR doesn't change that stay open on a server once lookups asked in them.
const VISITED: usize = 16;

/// A server asked to stop, whose end the caller may wait for.
pub(super) struct Stopping(Receiver<Event>);

impl Stopping {
    /// Waits until the server's process has ended, or until `until`. The events end once the client has
    /// killed it.
    pub(super) fn wait(self, until: Instant) {
        let _exit = exit_by(&self.0, until);
    }
}

/// How the server whose `events` these are exited, once it has, waiting until `until` at most. Nothing if
/// it hasn't by then, or its events ended without it, as when it was killed.
fn exit_by(events: &Receiver<Event>, until: Instant) -> Option<Exit> {
    while let Ok(event) = events.recv_timeout(until.saturating_duration_since(Instant::now())) {
        if let Event::Exited(exit) = event {
            return Some(exit);
        }
    }
    None
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
    /// holds with its text there. Its error output goes to its log beside the checkout, which keeps the
    /// last one as the old log. `news` hears each time its index moves on, and when it exits.
    pub(super) fn start(
        name: ServerName,
        program: &Path,
        (checkout, changed): (LookupCheckout, &[String]),
        news: OnNews,
    ) -> Result<Self, NotReady> {
        let (folder, _root) = located(&checkout.folder)?;
        let didnt_start = |problem: Error| {
            NotReady::Problem(ServerProblem::DidNotStart {
                cause: problem.message,
            })
        };
        let setup = match name {
            ServerName::Ty => super::ty::setup(&folder),
            ServerName::RustAnalyzer => {
                super::rust_analyzer::setup(program).map_err(didnt_start)?
            }
            ServerName::Clangd => super::clangd::setup(&folder, program).map_err(didnt_start)?,
        };
        let log = (new_log(&server_log(&folder, name)))
            .map(|file| -> Box<dyn Write + Send> { Box::new(file) });
        let launch = Launch {
            program: program.to_owned(),
            args: setup.args.clone(),
            folder,
            env: setup.env.clone(),
        };
        let started = Server::start(&launch, log).map_err(|cause| {
            NotReady::Problem(ServerProblem::DidNotStart {
                cause: cause.to_string(),
            })
        })?;
        Self::initialize((setup, started), (checkout, changed), news)
    }

    /// Initializes a server with `setup`, through the client that talks to it and its events, on
    /// `checkout`, and opens each of the files at `changed` the head holds with its text there.
    /// [`LanguageServer::start`] spawns the server, and tests talk to a fake one over in-memory pipes. A
    /// server that ends before it is ready says how it stopped, if the client hears within moments.
    pub(super) fn initialize(
        (setup, (starting, events)): (Setup, (Starting, Receiver<Event>)),
        (checkout, changed): (LookupCheckout, &[String]),
        news: OnNews,
    ) -> Result<Self, NotReady> {
        let LookupCheckout { folder, at } = checkout;
        let (folder, root) = located(&folder)?;
        let init = Init {
            root,
            options: setup.options,
            settings: setup.settings,
            experimental: setup.ready.experimental(),
            fallback: setup.fallback,
        };
        let (server, info) = match starting.initialize(init, PATIENCE) {
            Ok(started) => started,
            Err(cause) => return Err(not_started(&cause, &events)),
        };
        let version = (info.and_then(|info| info.version)).and_then(|text| version_in(&text));
        // git can't read a file whose path has a line break, so the server isn't told of it.
        let readable: Vec<(&String, [String; 2])> = (changed.iter())
            .filter_map(|path| {
                let name = |commit| diff::object_name(commit, path);
                Some((path, [name(&at.head)?, name(&at.base)?]))
            })
            .collect();
        let names: Vec<&str> = (readable.iter())
            .flat_map(|(_, names)| names.iter().map(String::as_str))
            .collect();
        let mut texts = diff::blobs(&folder, &names).into_iter();
        let mut open = Vec::new();
        for (path, _) in readable {
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
        if let Ready::Quiescent = setup.ready
            && let Some(exit) = quiescent(&events)
        {
            return Err(NotReady::Problem(ServerProblem::stopped(exit)));
        }
        let (writer, indexing) = latest::channel(Indexing::Built);
        let watched = Watched {
            index: setup.index,
            index_due: match &setup.troubles {
                None => IndexDue::Never,
                Some(troubles) if open.is_empty() => IndexDue::AfterOpen(troubles.index_within),
                Some(troubles) => IndexDue::from_now(troubles.index_within),
            },
            headers: setup.troubles.map(|troubles| {
                let texts = (open.iter())
                    .map(|open| {
                        let texts = [Some(&open.head), open.base.as_ref()].into_iter().flatten();
                        (open.document.uri().clone(), texts.cloned().collect())
                    })
                    .collect();
                Headers::new(troubles.program, (folder.clone(), at.head.clone()), texts)
            }),
        };
        let (events, watcher) = watch(events, watched, writer, news);
        let visited = match setup.unopened {
            Unopened::FromDisk => None,
            Unopened::Refused => Some(VecDeque::new()),
        };
        Ok(Self {
            server,
            events,
            version,
            folder,
            at,
            language: setup.language,
            open,
            visited,
            watcher,
            showing: DiffEnd::Head,
            used: Instant::now(),
            indexing,
        })
    }

    /// The server's progress with its index, which is [`Indexing::Ended`] once it has exited.
    pub(super) fn indexing(&self) -> Indexing {
        self.indexing.get()
    }

    /// The version the server named answering `initialize`, if it did.
    pub(super) fn version(&self) -> Option<String> {
        self.version.clone()
    }

    /// How the server exited, if it does within `patience`, as one whose answer failed as it ended. The
    /// server is gone either way.
    pub(super) fn exit_within(self, patience: Duration) -> Option<Exit> {
        exit_by(&self.events, Instant::now() + patience)
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
    /// turns its answer into hits. On clangd, a file the PR doesn't change opens first, if it isn't open.
    pub(super) fn ask(&mut self, request: &Request) -> (Pending<Definitions>, Hits) {
        self.used = Instant::now();
        let _unread = self.events.try_iter().count();
        self.show(request.side);
        self.visit(&request.document);
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

    /// On a server that refuses a lookup in a file that isn't open, opens `document` with its text in the
    /// checkout, unless it is open already. Its text is the same at the base and the head, so
    /// [`LanguageServer::show`] leaves it be. Once [`VISITED`] such files are open, the one asked about
    /// longest ago closes, and a file asked about again counts as the latest. A file the checkout can't give
    /// stays closed, so the next lookup in it tries again.
    ///
    /// It reads the file on the servers' thread, once per file while it stays open: a file of the checkout,
    /// on the local disk, as a start reads the PR's files from the Mirror on that thread too.
    fn visit(&mut self, document: &Uri) {
        let Some(visited) = self.visited.as_mut() else {
            return;
        };
        if (self.open.iter()).any(|open| open.document.uri() == document) {
            return;
        }
        let again = (visited.iter().position(|file| file.uri() == document))
            .and_then(|at| visited.remove(at));
        if let Some(file) = again {
            visited.push_back(file);
            return;
        }
        let Some(text) = document.to_path().and_then(|path| fs::read(path).ok()) else {
            return;
        };
        if visited.len() >= VISITED {
            // Dropping it sends `didClose`.
            let _closed = visited.pop_front();
        }
        let text = String::from_utf8_lossy(&text);
        visited.push_back(self.server.open(document.clone(), self.language, &text));
        // clangd reads its compile database, and starts its index, once a file is open on it.
        let _heard = self.watcher.send(Heard::Opened);
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

/// What a server's events are read for, besides its exit.
struct Watched {
    /// How far its index has got, from a `$/progress`, for a server whose index Tsugiki reads.
    index: Option<fn(&Progress) -> Option<Indexing>>,
    /// The moment clangd has to have reported its index by, once a file is open on it (#684).
    index_due: IndexDue,
    /// What clangd's diagnostics say of the headers it can't find (#684).
    headers: Option<Headers>,
}

/// The moment clangd has to have reported its index by (#684).
#[derive(Clone, Copy)]
enum IndexDue {
    /// Within this long of the first file a lookup opens on it, as it started with none open.
    AfterOpen(Duration),
    At(Instant),
    /// Never: the server isn't clangd, or the moment has passed.
    Never,
}

impl IndexDue {
    /// `within` from now.
    fn from_now(within: Duration) -> Self {
        Instant::now()
            .checked_add(within)
            .map_or(Self::Never, Self::At)
    }

    /// The moment, if one is set.
    fn at(self) -> Option<Instant> {
        match self {
            Self::At(due) => Some(due),
            Self::AfterOpen(_) | Self::Never => None,
        }
    }
}

/// A message to the thread that reads a server's events.
enum Heard {
    Event(Event),
    /// A lookup opened a file on the server.
    Opened,
    /// The server's events have ended.
    Ended,
}

/// Reads the server's `events` on a thread of their own as they come, and hands each on through the
/// receiver it gives. For a server whose index Tsugiki reads, it writes how far the index has got into
/// `indexing`, starting from [`Indexing::Unreported`], and tells `news` when that is news. A count it held
/// back as too soon it tells once [`NEWS_EVERY`] has passed, though the server says nothing more. It tells
/// `news` of a problem clangd's events show, or the lack of them. When the server exits, it writes
/// [`Indexing::Ended`] first, then tells `news` how. Once the server has exited, or its events end, it
/// writes [`Indexing::Ended`] and tells `news` that. The sender it gives tells the thread when a lookup
/// opens a file, which starts the wait for clangd's index if it is the first.
fn watch(
    events: Receiver<Event>,
    watched: Watched,
    mut indexing: LatestWriter<Indexing>,
    news: OnNews,
) -> (Receiver<Event>, Sender<Heard>) {
    let (passes, passed) = mpsc::channel();
    // A thread of their own hands the events on, so the watcher hears a file opened while it waits.
    let (hears, heard) = mpsc::channel();
    let forwards = hears.clone();
    let _forwards = thread::spawn(move || {
        let _gone = (events.into_iter().map(Heard::Event))
            .chain([Heard::Ended])
            .try_for_each(|heard| forwards.send(heard));
    });
    let first = if watched.index.is_some() {
        Indexing::Unreported
    } else {
        Indexing::Built
    };
    indexing.set(&first);
    let headers = (watched.headers).map(|headers| read_headers(headers, Arc::clone(&news)));
    let mut watcher = Watcher {
        index: watched.index,
        index_due: watched.index_due,
        headers,
        indexing,
        news,
        told: (first, Instant::now()),
        latest: first,
    };
    let _reads = thread::spawn(move || {
        loop {
            let event = match heard.recv_timeout(watcher.wait()) {
                Ok(Heard::Event(event)) => event,
                Ok(Heard::Opened) => {
                    watcher.opened();
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {
                    watcher.due();
                    continue;
                }
                Ok(Heard::Ended) | Err(RecvTimeoutError::Disconnected) => break,
            };
            watcher.read(&event);
            // Once a deadline has passed, events already queued would come first for as long as they last.
            watcher.due();
            let exited = matches!(event, Event::Exited(_));
            // The server is gone once nothing reads its events.
            if passes.send(event).is_err() || exited {
                break;
            }
        }
        watcher.ended();
    });
    (passed, hears)
}

/// Reads the diagnostics it is sent with `headers` on a thread of their own, which reads the disk and git,
/// and tells `news` of each problem they show, until nothing sends any more.
fn read_headers(mut headers: Headers, news: OnNews) -> Sender<Diagnostics> {
    let (sends, diagnosed) = mpsc::channel::<Diagnostics>();
    let _reads = thread::spawn(move || {
        for diagnostics in diagnosed {
            if let Some(problem) = headers.problem(&diagnostics) {
                news(News::Troubled(problem));
            }
        }
    });
    sends
}

/// The reader of a server's events, on their thread.
struct Watcher {
    /// How far its index has got, from a `$/progress`, for a server whose index Tsugiki reads.
    index: Option<fn(&Progress) -> Option<Indexing>>,
    /// The moment clangd has to have reported its index by.
    index_due: IndexDue,
    /// Where clangd's diagnostics go to be read for the headers it can't find.
    headers: Option<Sender<Diagnostics>>,
    indexing: LatestWriter<Indexing>,
    news: OnNews,
    /// The progress with its index last told, and when.
    told: (Indexing, Instant),
    /// Its progress with its index, as it last reported it.
    latest: Indexing,
}

impl Watcher {
    /// The wait until the next thing due: a count held back as too soon, or the moment clangd has to have
    /// reported its index by. A wait too long for a deadline waits for the next event.
    fn wait(&self) -> Duration {
        let news = (self.latest != self.told.0)
            .then(|| self.told.1.checked_add(NEWS_EVERY))
            .flatten();
        let due = news.into_iter().chain(self.index_due.at()).min();
        due.map_or(Duration::MAX, |due| {
            due.saturating_duration_since(Instant::now())
        })
    }

    /// Tells what is due by now: that clangd isn't indexing, if it hasn't reported its index yet, and a
    /// count held back as too soon.
    fn due(&mut self) {
        let now = Instant::now();
        if (self.index_due.at()).is_some_and(|due| due <= now) {
            self.index_due = IndexDue::Never;
            if self.latest == Indexing::Unreported {
                (self.news)(News::Troubled(ServerProblem::NotIndexing));
            }
        }
        if self.latest != self.told.0 && self.told.1.elapsed() >= NEWS_EVERY {
            self.told = (self.latest, now);
            (self.news)(News::Indexing);
        }
    }

    /// Notes that a lookup opened a file on the server, which starts the wait for clangd's index if it
    /// started with none open.
    fn opened(&mut self) {
        if let IndexDue::AfterOpen(within) = self.index_due {
            self.index_due = IndexDue::from_now(within);
        }
    }

    /// Reads `event`: how far the server's index has got, a header clangd can't find, or how it exited.
    fn read(&mut self, event: &Event) {
        if let Event::Progress(progress) = event
            && let Some(now) = self.index.and_then(|read| read(progress))
        {
            self.indexing.set(&now);
            self.latest = now;
            if now.news_since(self.told.0, self.told.1.elapsed()) {
                self.told = (now, Instant::now());
                (self.news)(News::Indexing);
            }
        }
        if let Event::Diagnostics(diagnostics) = event
            && let Some(headers) = &self.headers
        {
            // The thread that reads them ends only once this one has.
            let _sent = headers.send(diagnostics.clone());
        }
        if let Event::Exited(exit) = event {
            // First, so the servers' thread finds this server has ended when it hears.
            self.indexing.set(&Indexing::Ended);
            (self.news)(News::Exited(exit.clone()));
        }
    }

    /// Notes that the server has ended, and tells that.
    fn ended(mut self) {
        self.indexing.set(&Indexing::Ended);
        (self.news)(News::Indexing);
    }
}

/// Waits until the server says it is quiescent, or has ended, for at most [`PATIENCE`]. A server that is
/// still loading then answers what it can. One that has ended gives how.
fn quiescent(events: &Receiver<Event>) -> Option<Exit> {
    let until = Instant::now() + PATIENCE;
    let mut ended = None;
    while let Ok(event) = events.recv_timeout(until.saturating_duration_since(Instant::now())) {
        match event {
            Event::Status(Status {
                quiescent: true, ..
            }) => break,
            Event::Exited(exit) => {
                ended = Some(exit);
                break;
            }
            Event::Status(_) | Event::Progress(_) | Event::Diagnostics(_) => {}
        }
    }
    ended
}

/// Why a server whose `initialize` failed with `cause` didn't start: how it stopped, if it ended and the
/// client hears within [`EXIT_NEWS`], or else the cause.
fn not_started(cause: &RequestError, events: &Receiver<Event>) -> NotReady {
    let ended = matches!(cause, RequestError::Exited);
    let exit = ended.then(|| exit_by(events, Instant::now() + EXIT_NEWS));
    NotReady::Problem(match exit.flatten() {
        Some(exit) => ServerProblem::stopped(exit),
        None => ServerProblem::DidNotStart {
            cause: cause.to_string(),
        },
    })
}

/// A new log at `path` for a server that starts, once the last one is renamed to the old log beside it,
/// which that replaces. Without one, as when its folder can't be made, the server runs all the same.
fn new_log(path: &Path) -> Option<File> {
    let old = path.with_extension("old.log");
    let made = (path.parent().map_or(Ok(()), fs::create_dir_all))
        .and_then(|()| unless_missing(fs::rename(path, &old)))
        .and_then(|()| File::create(path));
    match made {
        Ok(file) => Some(file),
        Err(cause) => {
            let path = path.display();
            log::warn!("Tsugiki can't write a language server's log at {path}: {cause}");
            None
        }
    }
}

/// `result`, which counts a file that wasn't there as done.
fn unless_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}
