//! Where a name in the open PR's code is defined, which the Definition peek and Go to definition ask a
//! language server (ADR 0020). ty answers in Python, rust-analyzer in Rust, and clangd in C.
//!
//! Each Repository gets its own server for each language, which reads that Repository's **Lookup
//! checkout**: the PR's base on disk, with the files the PR adds. ty and clangd start when a PR that changes
//! a file in their language opens, and rust-analyzer on the first lookup in Rust. Each file the PR changes
//! in the server's language is open on it with its text at the head. A lookup from a removed line first
//! gives those files their text at the base, and the next lookup from another line puts the head's back. A
//! PR at other commits gets new servers, as its checkout has moved. A server that no lookup has reached for
//! the idle time stops until the next lookup starts it again, and a removed Repository's servers stop.
//!
//! One thread owns the servers, and each lookup is a message to it, so the window never waits on a server.
//! The thread sends the server the request and goes on, and a thread of the lookup's own waits for the
//! answer and hands it back as a message. A newer lookup, or a cancel, cancels the one running.
//!
//! clangd may answer while it builds its index, and name a declaration in a header for a function its index
//! doesn't hold yet. A thread of each clangd's own reads how far the index has got. The lookup that answered
//! then hears it, and once the index is built, the thread asks clangd again and hands the lookup the new
//! answer.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tsugiki_lsp::{Canceller, Pending, RequestError, Uri};

pub use crate::diff::DiffEnd;
use crate::diff::DiffFile;
use crate::error::{Error, ErrorKind};
use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;
use code::HitFiles;
pub use code::{Code, HitFile};
use server::{Hits, IndexMoved, Indexing, LanguageServer, Request, Stopping};

mod clangd;
mod code;
mod rust_analyzer;
mod server;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod ty;

/// The longest a server gets to start. A lookup has no time limit, as the user can cancel it.
const PATIENCE: Duration = Duration::from_secs(30);

/// The time a server asked to stop gets to agree, and again to exit after `exit`, before it is killed.
const EXIT_PATIENCE: Duration = Duration::from_secs(5);

/// The longest a server asked to stop takes to end: [`EXIT_PATIENCE`] to agree, as long again to exit, and a
/// second for the kill.
const ENDED_WITHIN: Duration = EXIT_PATIENCE
    .saturating_mul(2)
    .saturating_add(Duration::from_secs(1));

/// The file that marks a Repository with CPython's layout, whose servers Tsugiki sets up for it, relative to
/// the checkout.
const CPYTHON_MARK: &str = "Include/patchlevel.h";

/// A lookup the window asks for: where the name at `column` of line `line` of `path` is defined, with the
/// PR's files at `side`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub repository: RepositoryName,
    /// The diff's two ends, which the Lookup checkout holds.
    pub at: LookupAt,
    /// The file's path at `side`: a renamed file's old path at the base.
    pub path: String,
    pub side: DiffEnd,
    /// The line's number at `side`, from 1.
    pub line: u32,
    /// Where the name starts in the line, in UTF-16 code units. The window counts a line's text in them, as
    /// LSP does, so the column goes to the server as it is.
    pub column: u32,
}

/// Where a hit is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    /// A file of the Repository, at the PR's head for a file the PR changes when the lookup was at the head,
    /// and at its base for the rest.
    Repository {
        path: String,
        side: DiffEnd,
        #[specta(type = String)]
        commit: CommitSha,
    },
    /// A file outside the Repository, such as a stub of the standard library that ty keeps or a file of
    /// Rust's standard library, by its path on disk.
    Outside { path: String },
}

/// A place where the name is defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Hit {
    pub place: Place,
    /// Its line, from 1.
    pub line: u32,
    /// Where the definition's name starts in the line, in UTF-16 code units.
    pub column: u32,
    /// The line's text, as the server read it.
    pub text: String,
}

/// What a lookup found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Answer {
    /// The places the server named, in its order, which may be none.
    Found { hits: Vec<Hit> },
    /// The server for the file's language isn't set up.
    NotSetUp { server: ServerName },
    /// Tsugiki has no server for the file's language yet.
    NoServer,
    /// A newer lookup, or the user, cancelled it before the server answered.
    Cancelled,
}

/// A language server Tsugiki talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ServerName {
    /// ty, for Python.
    Ty,
    /// rust-analyzer, for Rust.
    RustAnalyzer,
    /// clangd, for C.
    Clangd,
}

impl ServerName {
    /// The server that answers lookups in the file at `path`, by its extension.
    fn of(path: &str) -> Option<Self> {
        if path.ends_with(".py") || path.ends_with(".pyi") {
            Some(Self::Ty)
        } else if path.ends_with(".rs") {
            Some(Self::RustAnalyzer)
        } else if path.ends_with(".c") || path.ends_with(".h") {
            Some(Self::Clangd)
        } else {
            None
        }
    }

    /// The server's name, as Tsugiki's messages say it.
    fn as_str(self) -> &'static str {
        match self {
            Self::Ty => "ty",
            Self::RustAnalyzer => "rust-analyzer",
            Self::Clangd => "clangd",
        }
    }
}

/// What a lookup waits for, which the Definition peek says while it waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Waiting {
    /// The server starts, as on the first lookup in a Repository.
    Starting { server: ServerName },
    /// The server works out the answer.
    Asking { server: ServerName },
}

/// The news of the index of the server that answered a lookup, which the Definition peek says under that
/// answer. Until clangd's index holds the file that defines a function, clangd names its declaration in a
/// header, so the core asks again once the index is built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Index {
    /// The server builds its index: the answer may miss what the index doesn't hold yet.
    Building {
        server: ServerName,
        /// The files it has indexed, once it has said.
        files: Option<IndexedFiles>,
    },
    /// The server has built its index, and the core asked it again: these are the places it names now, in
    /// place of the answer's.
    Built { hits: Vec<Hit> },
    /// The server stopped, or failed the lookup asked again, before it found more. The answer stands.
    Stopped,
}

/// The files a server has indexed of those it has to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct IndexedFiles {
    pub done: u32,
    pub total: u32,
}

/// The number the window gives a lookup, which a cancel names. The window may cancel a lookup before the
/// core has taken it, or after it has answered, so a cancel names the lookup it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
pub struct Ticket(pub u32);

/// Who hears what a lookup waits for, as that changes.
pub type OnWaiting = Box<dyn Fn(Waiting) + Send>;

/// The listener to how the index of the server that answered a lookup goes, while it builds it.
pub type OnIndex = Box<dyn Fn(Index) + Send>;

/// Where a Repository's Lookup checkout is, once it has stopped moving.
pub type FindCheckout = Box<dyn Fn(&RepositoryName) -> Option<LookupCheckout> + Send>;

/// What the servers' thread asks the rest of Tsugiki.
pub struct Sources {
    pub checkout: FindCheckout,
    /// Where each server is, if the user has set it up.
    pub servers: Box<dyn Fn(ServerName) -> Option<PathBuf> + Send>,
    /// The idle time: a server that no lookup reaches for this long stops.
    pub idle: Box<dyn Fn() -> Duration + Send>,
}

/// The handle the rest of Tsugiki asks the servers' thread through. The thread stops once the last handle
/// is gone.
#[derive(Clone)]
pub struct Definitions {
    messages: Arc<Messages>,
    /// The files [`Definitions::code`] reads.
    files: Arc<HitFiles>,
}

/// The way to the servers' thread, which stops it once the last handle drops it.
struct Messages(Sender<Message>);

impl Drop for Messages {
    fn drop(&mut self) {
        let (reply, _unheard) = mpsc::channel();
        // The thread may have stopped already.
        let _unheard = self.0.send(Message::Stop(PATIENCE, reply));
    }
}

impl Definitions {
    /// Starts the thread that owns the servers. It starts none until a PR asks for one, and ends on
    /// [`Definitions::stop`].
    pub fn start(sources: Sources) -> Self {
        let owner = Owner::new(sources);
        let messages = Arc::new(Messages(owner.own.clone()));
        let files = Arc::clone(&owner.files);
        let _owner = thread::spawn(move || owner.run());
        Self { messages, files }
    }

    /// Lines `lines` of the file a hit is in, counted from 1, which the Definition peek shows: one of the
    /// Repository whose Mirror is at `mirror`, at the hit's commit, or one outside it that a server named in
    /// a hit. It reads on the caller's thread, so it never waits for a lookup.
    pub fn code(&self, mirror: &Path, file: &HitFile, lines: Range<u32>) -> Result<Code, Error> {
        self.files.read(mirror, file, lines)
    }

    /// Notes the PR whose diff at `at` changes `files`, if a server reads one of them, and returns at once.
    /// ty starts for a PR that changes a Python file, once the Lookup checkout is at `at`.
    pub fn opened(&self, repository: RepositoryName, at: LookupAt, files: &[DiffFile]) {
        let changed = served_files(files);
        if !changed.is_empty() {
            self.send(Message::Opened(repository, Pr { at, changed }));
        }
    }

    /// Where `lookup`'s name is defined. It cancels the lookup running, if one is, and waits for the server,
    /// and for the server to start if it hasn't, so the window asks it from a thread of its own. `waiting`
    /// hears what it waits for as that changes, and a cancel that names `ticket` cancels it.
    ///
    /// A server that answers while it builds its index, as clangd does, may miss what the index doesn't hold
    /// yet. `index` then hears how far it has got, around the answer and as that changes, at most about once
    /// a second. Once the index is built, the core asks the server again, and `index` hears the places it
    /// names then as [`Index::Built`], or [`Index::Stopped`] if the server stops first. A newer lookup, or a
    /// cancel that names `ticket`, ends that.
    pub fn look_up(
        &self,
        lookup: Lookup,
        ticket: Ticket,
        (waiting, index): (OnWaiting, OnIndex),
    ) -> Result<Answer, Error> {
        let (reply, answer) = mpsc::channel();
        let asker = Asker {
            ticket,
            reply,
            waiting,
            index,
        };
        self.send(Message::LookUp(lookup, asker));
        let gone = Error::new(ErrorKind::Internal, "Tsugiki's lookups have stopped.");
        answer.recv().unwrap_or(Err(gone))
    }

    /// Cancels the lookup with `ticket`, whose answer is then [`Answer::Cancelled`]: the one running, or the
    /// next one the thread takes. A cancel for a lookup that has answered only stops it hearing how the
    /// server's index goes.
    pub fn cancel(&self, ticket: Ticket) {
        self.send(Message::Cancel(ticket));
    }

    /// Stops `repository`'s servers, cancels a lookup running on one, and waits until the servers' own
    /// processes have ended. Each gets [`EXIT_PATIENCE`] to agree and as long to exit. A process a server
    /// started may still be ending, so a delete of the Repository's folder retries.
    pub fn remove(&self, repository: RepositoryName) {
        let (reply, removed) = mpsc::channel();
        self.send(Message::Remove(repository, reply));
        // The thread may have stopped, and its servers with it.
        let _stopped = removed.recv();
    }

    /// Asks each server to stop, and waits up to `patience` for them to end. On Windows one still running
    /// when Tsugiki's process ends ends with it. The thread then ends, and a later lookup says so.
    pub fn stop(&self, patience: Duration) {
        let (reply, stopped) = mpsc::channel();
        self.send(Message::Stop(patience, reply));
        let _waited = stopped.recv_timeout(patience);
    }

    fn send(&self, message: Message) {
        // The thread ends once it has stopped, and the next lookup says so.
        let _unheard = self.messages.0.send(message);
    }
}

/// The paths at the head of the files in `files` that a server reads. A path with a line break can't be read
/// with `git cat-file --batch`, which reads a name a line.
fn served_files(files: &[DiffFile]) -> Vec<String> {
    (files.iter())
        .map(|file| &file.path)
        .filter(|path| ServerName::of(path).is_some() && !path.contains('\n'))
        .cloned()
        .collect()
}

enum Message {
    Opened(RepositoryName, Pr),
    LookUp(Lookup, Asker),
    Cancel(Ticket),
    /// The server's answer to the lookup numbered so, as the places it names, which a thread of the lookup's
    /// own hands back.
    Answered(u64, Result<Vec<Hit>, RequestError>),
    /// The index of the server the key names has moved on, perhaps a server stopped since.
    Indexing(Key),
    /// The Repository was removed: its servers stop, and the reply comes once they have ended.
    Remove(RepositoryName, Sender<()>),
    Stop(Duration, Sender<()>),
}

/// The lookup's ticket, where its answer goes, who hears what it waits for, and who hears how the
/// server's index goes.
struct Asker {
    ticket: Ticket,
    reply: Sender<Result<Answer, Error>>,
    waiting: OnWaiting,
    index: OnIndex,
}

impl Asker {
    fn answer(&self, answer: Result<Answer, Error>) {
        // The window may have stopped waiting.
        let _unheard = self.reply.send(answer);
    }
}

/// What became of a lookup the servers' thread took.
enum Sent {
    /// It has its answer, and no server was asked.
    Answered(Answer),
    /// A server was asked the request, and its answer comes back with this number. The canceller cancels
    /// it, and the server's index was so far along when it was asked.
    Asked(Request, (u64, Canceller), Indexing),
}

/// A Repository's server for one language.
type Key = (RepositoryName, ServerName);

/// The number of cancels that matched no lookup the servers' thread keeps. The window cancels a lookup that
/// has answered each time a peek closes, and a lookup it cancels before the core takes it comes within
/// moments.
const EARLY_CANCELS: usize = 16;

/// The servers that start when a PR that changes a file in their language opens. rust-analyzer waits for the
/// first lookup in Rust, as it reads the whole workspace as it starts.
const STARTED_BY_PR: [ServerName; 2] = [ServerName::Ty, ServerName::Clangd];

/// The lookup a server works on.
struct Running {
    /// The number its answer comes back with.
    id: u64,
    /// The server it asked.
    server: Key,
    canceller: Canceller,
    asked: Asked,
}

/// The time a server works on a lookup: the first, or once more with its index built.
enum Asked {
    /// The first, by `asker`, with the request the server was sent and its progress with its index then.
    First {
        asker: Asker,
        request: Request,
        indexing: Indexing,
    },
    /// Again, once the server has built its index, after the window has the first answer.
    Again(Listener),
}

impl Asked {
    /// The ticket of the lookup asked.
    fn ticket(&self) -> Ticket {
        match self {
            Self::First { asker, .. } => asker.ticket,
            Self::Again(listener) => listener.ticket,
        }
    }
}

impl Running {
    /// Cancels the lookup: the server stops working on it, and its asker hears it was cancelled, or that the
    /// index brings nothing more once it has its answer.
    fn cancel(self) {
        self.canceller.cancel();
        match self.asked {
            Asked::First { asker, .. } => asker.answer(Ok(Answer::Cancelled)),
            Asked::Again(listener) => (listener.index)(Index::Stopped),
        }
    }
}

/// A lookup that has its answer, and hears how the index of the server that answered goes: its ticket,
/// which a cancel names, and who hears.
struct Listener {
    ticket: Ticket,
    index: OnIndex,
}

/// A lookup that answered while its server built its index, or before it reported one, which hears how the
/// index goes until it is built and the core has asked again: the request the server was sent, and the
/// server.
struct Watch {
    listener: Listener,
    request: Request,
    server: Key,
}

/// A PR on screen: its diff's two ends, and the paths at the head of the files it changes that a server
/// reads.
#[derive(Debug, Clone)]
struct Pr {
    at: LookupAt,
    changed: Vec<String>,
}

impl Pr {
    /// The files it changes in `server`'s language.
    fn changed_for(&self, server: ServerName) -> Vec<String> {
        (self.changed.iter())
            .filter(|path| ServerName::of(path) == Some(server))
            .cloned()
            .collect()
    }
}

/// The servers' thread.
struct Owner {
    sources: Sources,
    /// The last PR each Repository opened that changes a file a server reads.
    prs: HashMap<RepositoryName, Pr>,
    servers: HashMap<Key, LanguageServer>,
    /// The files the peek reads, among them those outside the Repository that a hit has named.
    files: Arc<HitFiles>,
    /// The lookup a server works on, if one does.
    running: Option<Running>,
    /// The last lookup to answer, while its server builds its index.
    watching: Option<Watch>,
    /// The tickets of the last cancels that matched no lookup, newest last. One may have come before its
    /// lookup, which it cancels when it comes. Most are for lookups that have answered.
    cancelled: VecDeque<Ticket>,
    /// How many lookups the servers have been asked.
    asked: u64,
    /// The thread's own messages: those it takes, and the way the threads that wait for a server hand back
    /// its answers. Holding a sender keeps the channel open, so the thread ends only on a stop.
    own: Sender<Message>,
    incoming: Receiver<Message>,
    /// Messages taken early, to see whether a newer lookup came while a server started, and not handled yet.
    queued: VecDeque<Message>,
}

impl Owner {
    fn new(sources: Sources) -> Self {
        let (own, incoming) = mpsc::channel();
        Self {
            sources,
            prs: HashMap::new(),
            servers: HashMap::new(),
            files: Arc::default(),
            running: None,
            watching: None,
            cancelled: VecDeque::new(),
            asked: 0,
            own,
            incoming,
            queued: VecDeque::new(),
        }
    }

    fn run(mut self) {
        while let Some(message) = self.next() {
            if !self.handle(message) {
                return;
            }
        }
    }

    /// The next message to handle, which waits for one. While it waits, each server that no lookup has
    /// reached for the idle time stops.
    fn next(&mut self) -> Option<Message> {
        if let Some(message) = self.queued.pop_front() {
            return Some(message);
        }
        loop {
            let (now, idle) = (Instant::now(), (self.sources.idle)());
            self.stop_idle(now, idle);
            let Some(wait) = self.until_idle(now, idle) else {
                return self.incoming.recv().ok();
            };
            match self.incoming.recv_timeout(wait) {
                Err(RecvTimeoutError::Timeout) => {}
                received @ (Ok(_) | Err(RecvTimeoutError::Disconnected)) => return received.ok(),
            }
        }
    }

    /// Handles `message`, and says whether the thread goes on.
    fn handle(&mut self, message: Message) -> bool {
        match message {
            Message::Opened(repository, pr) => self.opened(&repository, pr),
            Message::LookUp(lookup, asker) => self.look_up(lookup, asker),
            Message::Cancel(ticket) => self.cancel(ticket),
            Message::Answered(id, hits) => self.answered(id, hits),
            Message::Indexing(key) => self.indexing(&key),
            Message::Remove(repository, reply) => {
                self.remove(&repository);
                let _unheard = reply.send(());
            }
            Message::Stop(patience, reply) => {
                self.cancel_running();
                let all: Vec<Key> = self.servers.keys().cloned().collect();
                wait_all(self.stop_servers(&all), Instant::now() + patience);
                let _unheard = reply.send(());
                return false;
            }
        }
        true
    }

    /// Stops `repository`'s servers and forgets its PR, cancels the lookup running on one of them, and waits
    /// until the servers' own processes have ended.
    fn remove(&mut self, repository: &RepositoryName) {
        let _pr = self.prs.remove(repository);
        let theirs: Vec<Key> = (self.servers.keys())
            .filter(|(owner, _)| owner == repository)
            .cloned()
            .collect();
        wait_all(self.stop_servers(&theirs), Instant::now() + ENDED_WITHIN);
    }

    /// Asks the servers `keys` name to stop and forgets them, and cancels the lookup running on one of them.
    /// A lookup that hears how the index of one goes hears nothing more.
    fn stop_servers(&mut self, keys: &[Key]) -> Vec<Stopping> {
        let asked = self
            .running
            .take_if(|running| keys.contains(&running.server));
        asked.into_iter().for_each(Running::cancel);
        for key in keys {
            self.unwatch(key);
        }
        (keys.iter())
            .filter_map(|key| self.servers.remove(key))
            .map(LanguageServer::stop)
            .collect()
    }

    /// Stops each server that no lookup has reached for `idle` by `now`, but the one the lookup running
    /// asked, and one whose index a lookup watches. The next lookup in its language starts it again.
    fn stop_idle(&mut self, now: Instant, idle: Duration) {
        let stale: Vec<Key> = (self.servers.iter())
            .filter(|(key, server)| !self.busy(key) && now.duration_since(server.used()) >= idle)
            .map(|(key, _)| key.clone())
            .collect();
        let minutes = idle.as_secs() / 60;
        for (repository, name) in &stale {
            let named = name.as_str();
            log::info!(
                "Tsugiki stops {named} for {repository}, which no lookup has reached for {minutes} minutes"
            );
        }
        let _stopping = self.stop_servers(&stale);
    }

    /// The wait from `now` until the next server has gone `idle` without a lookup, if a server can.
    fn until_idle(&self, now: Instant, idle: Duration) -> Option<Duration> {
        (self.servers.iter())
            .filter(|(key, _)| !self.busy(key))
            .map(|(_, server)| {
                (server.used().checked_add(idle))
                    .map_or(Duration::MAX, |at| at.saturating_duration_since(now))
            })
            .min()
    }

    /// Starts the servers that start with a PR for `pr`, in each language it changes a file in, unless `pr`
    /// is the Repository's PR already. The Files tab reads its PR again on each change, and a server that
    /// failed to start, or a checkout that failed to move, waits for a lookup to try again.
    fn opened(&mut self, repository: &RepositoryName, pr: Pr) {
        if self
            .prs
            .get(repository)
            .is_some_and(|known| known.at == pr.at)
        {
            return;
        }
        let _old = self.prs.insert(repository.clone(), pr.clone());
        self.stop_stale(repository, &pr.at);
        for name in STARTED_BY_PR {
            if !pr.changed_for(name).is_empty() {
                self.start(&(repository.clone(), name), &pr);
            }
        }
    }

    /// Stops `repository`'s servers that wait for a lookup to start and read other commits than `at`, as the
    /// checkout moves away from what they read, and cancels the lookup running on one. The next lookup in
    /// their language starts one at `at`.
    fn stop_stale(&mut self, repository: &RepositoryName, at: &LookupAt) {
        let stale: Vec<Key> = (self.servers.iter())
            .filter(|((owner, name), server)| {
                owner == repository && !STARTED_BY_PR.contains(name) && server.at() != at
            })
            .map(|(key, _)| key.clone())
            .collect();
        let _stopping = self.stop_servers(&stale);
    }

    /// Starts the server `key` names for `pr`, if the user has set it up.
    fn start(&mut self, key: &Key, pr: &Pr) {
        let (repository, name) = key;
        let Some(program) = (self.sources.servers)(*name) else {
            return;
        };
        let replaces = (self.servers.get(key)).is_some_and(|server| *server.at() != pr.at);
        let checkout = (&self.sources.checkout, &self.own);
        let started = ready(&mut self.servers, checkout, (key, pr), (&program, &|_| {}));
        if let Err(problem) = started {
            let named = name.as_str();
            let why = problem.message;
            log::warn!("Tsugiki couldn't start {named} for {repository}: {why}");
        }
        // The old server stopped, unless the checkout wasn't at `pr` yet, whether or not the new one started.
        let stopped = (self.servers.get(key)).is_none_or(|server| *server.at() == pr.at);
        if replaces && stopped {
            if self.asks(key) {
                self.cancel_running();
            }
            self.unwatch(key);
        }
    }

    /// Tells the lookup that hears how the index of the server `key` names goes that the server stopped, and
    /// stops telling it more.
    fn unwatch(&mut self, key: &Key) {
        if let Some(watch) = (self.watching).take_if(|watch| watch.server == *key) {
            (watch.listener.index)(Index::Stopped);
        }
    }

    /// Cancels the lookup running, if one is, and sends the server for the lookup's language the request for
    /// `lookup`, unless a cancel for it came first, or a newer lookup or its cancel came while the server
    /// started. Its answer comes back as [`Message::Answered`]. The lookup that heard how an index goes
    /// hears nothing more. A lookup cancelled before it came leaves the others be, as it may come late.
    fn look_up(&mut self, lookup: Lookup, asker: Asker) {
        if let Some(at) = self
            .cancelled
            .iter()
            .position(|ticket| *ticket == asker.ticket)
        {
            let _taken = self.cancelled.remove(at);
            asker.answer(Ok(Answer::Cancelled));
            return;
        }
        self.cancel_running();
        self.watching = None;
        let Some(name) = ServerName::of(&lookup.path) else {
            asker.answer(Ok(Answer::NoServer));
            return;
        };
        let key = (lookup.repository.clone(), name);
        match self.send(&key, &lookup, &asker) {
            Ok(Sent::Asked(request, (id, canceller), indexing)) => {
                self.running = Some(Running {
                    id,
                    server: key,
                    canceller,
                    asked: Asked::First {
                        asker,
                        request,
                        indexing,
                    },
                });
            }
            Ok(Sent::Answered(answer)) => asker.answer(Ok(answer)),
            Err(problem) => asker.answer(Err(problem)),
        }
    }

    /// Sends the server `key` names the request for `lookup`, starting the server if it has to, and tells
    /// `asker` what it waits for.
    fn send(&mut self, key: &Key, lookup: &Lookup, asker: &Asker) -> Result<Sent, Error> {
        let (repository, name) = key;
        let Some(program) = (self.sources.servers)(*name) else {
            return Ok(Sent::Answered(Answer::NotSetUp { server: *name }));
        };
        let unknown = Pr {
            at: lookup.at.clone(),
            changed: Vec::new(),
        };
        let pr = (self.prs.get(repository))
            .filter(|pr| pr.at == lookup.at)
            .cloned()
            .unwrap_or(unknown);
        let starts = !self.runs(key, &pr.at);
        let checkout = (&self.sources.checkout, &self.own);
        let waiting: &dyn Fn(Waiting) = &asker.waiting;
        let start = (program.as_path(), waiting);
        let server = ready(&mut self.servers, checkout, (key, &pr), start)?;
        if starts {
            // The user may have asked again, or cancelled, while the server started.
            self.queued.extend(self.incoming.try_iter());
            let newer = |message: &Message| match message {
                Message::LookUp(..) => true,
                Message::Cancel(ticket) => *ticket == asker.ticket,
                Message::Opened(..)
                | Message::Answered(..)
                | Message::Indexing(..)
                | Message::Remove(..)
                | Message::Stop(..) => false,
            };
            if self.queued.iter().any(newer) {
                return Ok(Sent::Answered(Answer::Cancelled));
            }
        }
        (asker.waiting)(Waiting::Asking { server: *name });
        let request = server.request(lookup)?;
        let (pending, hits) = server.ask(&request);
        let indexing = server.indexing();
        let waits = self.wait_for(pending, hits);
        Ok(Sent::Asked(request, waits, indexing))
    }

    /// Numbers the request `pending` waits on, and hands its answer back as [`Message::Answered`] with that
    /// number, from a thread of its own, as `hits` reads it. It gives the number, and what cancels it.
    fn wait_for(
        &mut self,
        pending: Pending<tsugiki_lsp::Definitions>,
        hits: Hits,
    ) -> (u64, Canceller) {
        self.asked += 1;
        let id = self.asked;
        let canceller = pending.canceller();
        let own = self.own.clone();
        let _waits = thread::spawn(move || {
            let answer = pending.wait().map(|definitions| hits.of(definitions));
            // The thread ends once it has stopped.
            let _unheard = own.send(Message::Answered(id, answer));
        });
        (id, canceller)
    }

    /// Hands the asker of the lookup numbered `id` the server's answer, unless it was cancelled since. An
    /// answer to a lookup asked again once the index was built goes to whoever hears how the index goes.
    fn answered(&mut self, id: u64, hits: Result<Vec<Hit>, RequestError>) {
        let Some(running) = self.running.take_if(|running| running.id == id) else {
            return;
        };
        let Running { server, asked, .. } = running;
        let name = server.1.as_str();
        let (hits, asked) = match (hits, asked) {
            (Ok(hits), asked) => (hits, asked),
            (Err(cause), Asked::First { asker, .. }) => {
                // A server that failed starts afresh on the next lookup.
                let _failed = self.servers.remove(&server);
                asker.answer(Err(failed(&format!("{name} couldn't answer: {cause}"))));
                return;
            }
            (Err(cause), Asked::Again(listener)) => {
                // The server keeps the index it has built for the next lookup.
                log::warn!("{name} couldn't answer a lookup asked again: {cause}");
                (listener.index)(Index::Stopped);
                return;
            }
        };
        if let Some(server) = self.servers.get_mut(&server) {
            server.answered();
        }
        // The peek may read a file outside the Repository once a hit names it.
        for hit in &hits {
            if let Place::Outside { path } = &hit.place {
                self.files.name(PathBuf::from(path));
            }
        }
        match asked {
            Asked::Again(listener) => (listener.index)(Index::Built { hits }),
            Asked::First {
                asker,
                request,
                indexing,
            } => {
                asker.answer(Ok(Answer::Found { hits }));
                let listener = Listener {
                    ticket: asker.ticket,
                    index: asker.index,
                };
                let watch = Watch {
                    listener,
                    request,
                    server,
                };
                self.watch(watch, indexing);
            }
        }
    }

    /// Watches the index of the server that answered `watch`'s lookup, if the answer may miss what the index
    /// didn't hold: the server builds its index now, or hasn't reported one yet, or built it since it was
    /// asked, as `asked` says. While the server builds its index, the lookup hears how far it has got. The
    /// lookup is asked again once the index is built, and at once if it was built since it was asked.
    fn watch(&mut self, watch: Watch, asked: Indexing) {
        match (asked, self.indexing_of(&watch.server)) {
            // clangd without a compile database never reports an index, and then nothing comes of this.
            (_, now @ (Indexing::Unreported | Indexing::Building(_))) => {
                self.watching = Some(watch);
                self.tell(now);
            }
            (Indexing::Unreported | Indexing::Building(_), Indexing::Built) => {
                self.ask_again(watch)
            }
            (Indexing::Built | Indexing::Ended, Indexing::Built) | (_, Indexing::Ended) => {}
        }
    }

    /// Tells the lookup that watches the index of the server `key` names how far it has got. Once the index
    /// is built, it asks the lookup again, and once the server has ended, it says so.
    fn indexing(&mut self, key: &Key) {
        if !(self.watching.as_ref()).is_some_and(|watch| watch.server == *key) {
            return;
        }
        match self.indexing_of(key) {
            now @ (Indexing::Unreported | Indexing::Building(_)) => self.tell(now),
            Indexing::Built => {
                if let Some(watch) = self.watching.take() {
                    self.ask_again(watch);
                }
            }
            Indexing::Ended => self.unwatch(key),
        }
    }

    /// Tells the lookup that watches an index that it is `now`, if the server has begun to build it.
    fn tell(&self, now: Indexing) {
        if let (Some(watch), Indexing::Building(files)) = (&self.watching, now) {
            (watch.listener.index)(Index::Building {
                server: watch.server.1,
                files,
            });
        }
    }

    /// Asks the server again for `watch`'s lookup, now that it has built its index. Its answer goes to
    /// whoever hears how the index goes, as [`Index::Built`].
    fn ask_again(&mut self, watch: Watch) {
        let Watch {
            listener,
            request,
            server: key,
        } = watch;
        // The caller read the server's index, so the server runs.
        let asked = (self.servers.get_mut(&key)).map(|server| server.ask(&request));
        if let Some((pending, hits)) = asked {
            let (id, canceller) = self.wait_for(pending, hits);
            self.running = Some(Running {
                id,
                server: key,
                canceller,
                asked: Asked::Again(listener),
            });
        }
    }

    /// The progress of the index of the server `key` names. One that has stopped has ended.
    fn indexing_of(&self, key: &Key) -> Indexing {
        (self.servers.get(key)).map_or(Indexing::Ended, LanguageServer::indexing)
    }

    /// Cancels the lookup with `ticket`: the one running, or the one that watches an index, or else the
    /// lookup with that ticket when it comes, if it hasn't come yet.
    fn cancel(&mut self, ticket: Ticket) {
        let watched = (self.watching)
            .take_if(|watch| watch.listener.ticket == ticket)
            .is_some();
        if (self.running.as_ref()).is_some_and(|running| running.asked.ticket() == ticket) {
            self.cancel_running();
        } else if !watched {
            self.cancelled.push_back(ticket);
            let older = self.cancelled.len().saturating_sub(EARLY_CANCELS);
            self.cancelled.drain(..older).for_each(drop);
        }
    }

    /// Cancels the lookup running, if one is: the server stops working on it, and its asker hears it was
    /// cancelled.
    fn cancel_running(&mut self) {
        self.running.take().into_iter().for_each(Running::cancel);
    }

    /// Whether the lookup running asked the server `key` names.
    fn asks(&self, key: &Key) -> bool {
        (self.running.as_ref()).is_some_and(|running| running.server == *key)
    }

    /// Whether a lookup needs the server `key` names: the one running asked it, or one watches its index.
    fn busy(&self, key: &Key) -> bool {
        self.asks(key) || (self.watching.as_ref()).is_some_and(|watch| watch.server == *key)
    }

    /// Whether the server `key` names reads `at`'s checkout.
    fn runs(&self, key: &Key, at: &LookupAt) -> bool {
        (self.servers.get(key)).is_some_and(|server| server.at() == at)
    }
}

/// The server in `servers` that `key` names and that reads `pr`, started at `program` if none does, which
/// `waiting` hears. A server that reads another PR's checkout stops once the checkout is at `pr`, and goes on
/// while it isn't. A server that starts tells the thread at `own` each time its index moves on.
fn ready<'a>(
    servers: &'a mut HashMap<Key, LanguageServer>,
    (checkout, own): (&FindCheckout, &Sender<Message>),
    (key, pr): (&Key, &Pr),
    (program, waiting): (&Path, &dyn Fn(Waiting)),
) -> Result<&'a mut LanguageServer, Error> {
    let (repository, name) = key;
    let server = match servers.remove(key) {
        Some(running) if *running.at() == pr.at => running,
        old => {
            let Some(checkout) = checkout(repository).filter(|checkout| checkout.at == pr.at)
            else {
                if let Some(old) = old {
                    let _none = servers.insert(key.clone(), old);
                }
                let elsewhere = format!(
                    "Tsugiki has no copy of this PR's files for {} to read yet.",
                    name.as_str()
                );
                return Err(Error::new(ErrorKind::NotFound, &elsewhere));
            };
            waiting(Waiting::Starting { server: *name });
            // Asked first, the old server can end while the new one starts.
            let _stopping = old.map(LanguageServer::stop);
            let changed = pr.changed_for(*name);
            LanguageServer::start(*name, program, (checkout, &changed), index_moved(own, key))?
        }
    };
    Ok(servers.entry(key.clone()).insert_entry(server).into_mut())
}

/// Tells the thread at `own` that the index of the server `key` names has moved on.
fn index_moved(own: &Sender<Message>, key: &Key) -> IndexMoved {
    let (own, key) = (own.clone(), key.clone());
    Box::new(move || {
        // The thread ends once it has stopped.
        let _unheard = own.send(Message::Indexing(key.clone()));
    })
}

/// Waits until each of the `stopping` servers has ended, or until `until`.
fn wait_all(stopping: Vec<Stopping>, until: Instant) {
    for server in stopping {
        server.wait(until);
    }
}

/// `path` without its `.` and `..` parts, as a server names the files it read.
fn plain(path: &Path) -> PathBuf {
    let mut plain = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            let _parent = plain.pop();
        } else if part != Component::CurDir {
            plain.push(part);
        }
    }
    plain
}

/// `relative` with `/` between its parts, as git and the diff name a path.
fn slashed(relative: &Path) -> String {
    let parts: Vec<_> = relative.iter().map(|part| part.to_string_lossy()).collect();
    parts.join("/")
}

fn uri(path: &Path) -> Result<Uri, Error> {
    let relative = Error::new(
        ErrorKind::Internal,
        "The Lookup checkout isn't at a full path.",
    );
    Uri::from_path(path).ok_or(relative)
}

fn failed(message: &str) -> Error {
    Error::new(ErrorKind::Internal, message)
}
