//! Where a name in the open PR's code is defined, which the Definition peek and Go to definition ask a
//! language server (ADR 0020). ty answers in Python, rust-analyzer in Rust, and clangd in C.
//!
//! Each Repository gets its own server for each language, which reads that Repository's **Lookup
//! checkout**: the PR's base on disk, with the files the PR adds. ty and clangd start when a PR that changes
//! a file in their language opens, and rust-analyzer on the first lookup in Rust. Each file the PR changes
//! in the server's language is open on it with its text at the head. clangd, which answers only in an open
//! file, also has the last 16 other files a lookup asked in open, with their text in the checkout. A lookup
//! from a removed line first gives the PR's files their text at the base, and the next lookup from another
//! line puts the head's back. A PR at other commits gets new servers, as its checkout has moved. A server
//! that no lookup has reached for the idle time stops until the next lookup starts it again, and a removed
//! Repository's servers stop.
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
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tsugiki_lsp::{Canceller, Exit, Pending, RequestError, Uri};

pub use crate::diff::{DiffEnd, HitFile, HitLines};
use crate::diff::{DiffFile, Highlighter};
use crate::error::{Error, ErrorKind};
use crate::facts::CommitSha;
use crate::folders::servers_beside;
use crate::highlight::Span;
use crate::latest::{self, LatestReader, LatestWriter};
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;
pub use code::Code;
use code::HitFiles;
use health::{Found, Health, Last, Presence};
pub use health::{FoundVersion, ServerProblem, ServerRow, ServerStatus};
use server::{Hits, Indexing, LanguageServer, News, OnNews, Request, Stopping};

mod clangd;
mod code;
mod health;
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
    /// The server for the file's language has a problem, which its row in Settings › Advanced says too:
    /// one that kept it from answering, which the next lookup tries again, or one of a clangd that runs,
    /// which found nothing and whose problem may say why (#684).
    Problem {
        server: ServerName,
        problem: ServerProblem,
    },
    /// Tsugiki has no server for the file's language yet.
    NoServer,
    /// A newer lookup, or the user, cancelled it before the server answered.
    Cancelled,
}

/// A language server Tsugiki talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
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
    /// Each server, in the order Settings › Advanced lists them.
    pub const ALL: [Self; 3] = [Self::Ty, Self::Clangd, Self::RustAnalyzer];

    /// The versions we tested, which `scripts/test-servers.ts` fetches or `rust-toolchain.toml` pins.
    fn tested(self) -> &'static [&'static str] {
        match self {
            Self::Ty => &["0.0.103"],
            Self::RustAnalyzer => &["1.99.1"],
            Self::Clangd => &["23.1.0"],
        }
    }

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
    pub fn as_str(self) -> &'static str {
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
    /// The server stopped, or failed the lookup asked again, before it found more, or found nothing again
    /// while it has a problem that says why (#684). The answer stands.
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
    /// Where each server is, if the user has set it up. The servers' thread reads this as it starts, and
    /// again on [`Definitions::reconfigured`], and starts servers from what it read last.
    pub servers: Box<dyn Fn(ServerName) -> Option<PathBuf> + Send>,
    /// The idle time: a server that no lookup reaches for this long stops.
    pub idle: Box<dyn Fn() -> Duration + Send>,
    /// Who hears the servers' rows in Settings › Advanced each time one changes.
    pub changed: OnRows,
}

/// The listener to the servers' rows in Settings › Advanced.
pub type OnRows = Box<dyn Fn(&[ServerRow]) + Send>;

/// Where server `server` of the Repository whose Lookup checkout is at `lookup` writes its error output:
/// `servers\<server>\<server>.log` beside the checkout. Each start keeps the last one as `<server>.old.log`.
pub fn server_log(lookup: &Path, server: ServerName) -> PathBuf {
    let name = server.as_str();
    servers_beside(lookup)
        .join(name)
        .join(format!("{name}.log"))
}

/// The handle the rest of Tsugiki asks the servers' thread through. The thread stops once the last handle
/// is gone.
#[derive(Clone)]
pub struct Definitions {
    messages: Arc<Messages>,
    /// The files [`Definitions::code`] reads.
    files: Arc<HitFiles>,
    /// The servers' rows in Settings › Advanced, as the thread last wrote them.
    rows: LatestReader<Vec<ServerRow>>,
}

/// The way to the servers' thread, which stops it once the last handle drops it.
struct Messages(Sender<Message>);

impl Drop for Messages {
    fn drop(&mut self) {
        // Nobody waits for the thread to stop.
        let (reply, _) = mpsc::channel();
        tell(&self.0, Message::Stop(PATIENCE, reply));
    }
}

impl Definitions {
    /// Starts the thread that owns the servers. It starts none until a PR asks for one, and ends on
    /// [`Definitions::stop`].
    pub fn start(sources: Sources) -> Self {
        let (owner, rows) = Owner::new(sources);
        let messages = Arc::new(Messages(owner.own.clone()));
        let files = Arc::clone(&owner.files);
        #[expect(
            unused_results,
            reason = "it ends on `Message::Stop`, which `stop` waits for with a patience a join can't take"
        )]
        thread::spawn(move || owner.run());
        Self {
            messages,
            files,
            rows,
        }
    }

    /// Each server's row in Settings › Advanced, as the servers' thread last wrote them. This never waits.
    pub fn rows(&self) -> Vec<ServerRow> {
        self.rows.get()
    }

    /// Tells the servers' thread that the settings may have changed, and returns at once. A server whose
    /// path changed stops, so the next lookup starts the new one, and a new path is checked with
    /// `--version` on a thread of its own. The idle time applies from the thread's next idle check, which
    /// this brings on.
    pub fn reconfigured(&self) {
        self.send(Message::Reconfigured);
    }

    /// The lines of the file a hit is in that `wanted` names, which the Definition peek shows: of a file of
    /// the Repository whose Mirror is at `mirror`, at the hit's commit, or of one outside it that a server
    /// named in a hit. It reads on the caller's thread, so it never waits for a lookup.
    pub fn code(&self, mirror: &Path, wanted: &HitLines) -> Result<Code, Error> {
        self.files.read(mirror, wanted)
    }

    /// The spans of the lines [`Definitions::code`] reads, which `highlighter` reads from the file's whole
    /// tree (#455). It waits for the parse of a file `highlighter` doesn't keep, but never for a lookup.
    pub fn spans(
        &self,
        highlighter: &Highlighter,
        mirror: &Path,
        wanted: &HitLines,
    ) -> Result<Vec<Vec<Span>>, Error> {
        self.files.spans(highlighter, mirror, wanted)
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
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the thread has stopped, and its servers with it"
        )]
        let _ = removed.recv();
    }

    /// Asks each server to stop, and waits up to `patience` for them to end. On Windows one still running
    /// when Tsugiki's process ends ends with it. The thread then ends, and a later lookup says so.
    pub fn stop(&self, patience: Duration) {
        let (reply, stopped) = mpsc::channel();
        self.send(Message::Stop(patience, reply));
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a server still running past `patience` ends with Tsugiki's process, and a thread that \
                      has gone has stopped its servers"
        )]
        let _ = stopped.recv_timeout(patience);
    }

    /// Sends the thread `message`. A thread that has stopped can't answer, and the next lookup says so.
    fn send(&self, message: Message) {
        tell(&self.messages.0, message);
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
    /// A server the key names exited, perhaps on its own, perhaps one stopped since.
    Exited(Key, Exit),
    /// What a server the key names reports, or doesn't, says it has this problem while it runs (#684).
    Troubled(Key, ServerProblem),
    /// The settings may have changed: a server's path, or the idle time.
    Reconfigured,
    /// What checking the program at the path found of the server, once its path was set.
    Checked(ServerName, PathBuf, Result<Found, ServerProblem>),
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
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the window has stopped waiting, and then nobody wants the answer"
        )]
        let _ = self.reply.send(answer);
    }
}

/// Why a server isn't there to ask.
#[derive(Debug)]
enum NotReady {
    /// The server has a problem, which its row says too.
    Problem(ServerProblem),
    /// Something else failed, such as the Lookup checkout not being at the PR.
    Failed(Error),
}

impl From<Error> for NotReady {
    fn from(failed: Error) -> Self {
        Self::Failed(failed)
    }
}

/// The longest a lookup whose server ended before it answered waits to hear how the server ended. The
/// client hears it once the server's error output has closed, which it waits a second for.
const EXIT_NEWS: Duration = Duration::from_secs(3);

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
    /// Each server's path as the thread last read it from the settings, which it starts the server from,
    /// and which [`Message::Reconfigured`] reads again.
    known: HashMap<ServerName, Option<PathBuf>>,
    /// Each server's version and what its `--version` check said, whichever Repository runs it.
    health: HashMap<ServerName, Health>,
    /// What last happened to each Repository's server. A server with nothing here has nothing to say.
    lasts: HashMap<Key, Last>,
    /// The Repository whose servers the rows show: the last to open a PR that changes a file a server reads,
    /// or to look a name up. That is the window's open Repository, from the time it does either.
    shown: Option<RepositoryName>,
    /// The servers' rows as the thread last wrote them, and where it writes them.
    told: Vec<ServerRow>,
    rows: LatestWriter<Vec<ServerRow>>,
}

impl Owner {
    /// The servers' thread, which hasn't started a server yet, and the rows it writes.
    fn new(sources: Sources) -> (Self, LatestReader<Vec<ServerRow>>) {
        let (own, incoming) = mpsc::channel();
        let known = (ServerName::ALL.into_iter())
            .map(|name| (name, (sources.servers)(name)))
            .collect();
        let (rows, reader) = latest::channel(Vec::new());
        let mut owner = Self {
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
            known,
            health: HashMap::new(),
            lasts: HashMap::new(),
            shown: None,
            told: Vec::new(),
            rows,
        };
        owner.publish();
        (owner, reader)
    }

    /// Writes each server's row, and tells the listener, if one has changed since the last time.
    fn publish(&mut self) {
        let rows: Vec<ServerRow> = (ServerName::ALL.into_iter())
            .map(|name| self.row(name))
            .collect();
        if rows != self.told {
            self.rows.set(&rows);
            (self.sources.changed)(&rows);
            self.told = rows;
        }
    }

    /// The row of server `name` in Settings › Advanced, as it stands in the Repository the rows show.
    fn row(&self, name: ServerName) -> ServerRow {
        let key = (self.shown.clone()).map(|repository| (repository, name));
        let presence = if self.program(name).is_none() {
            Presence::NotSetUp
        } else if (key.as_ref()).is_some_and(|key| self.servers.contains_key(key)) {
            Presence::Running
        } else {
            Presence::Stopped
        };
        let last = (key.and_then(|key| self.lasts.get(&key))).unwrap_or(&Last::Nothing);
        let health = self.health.get(&name).cloned().unwrap_or_default();
        health.row(name, presence, last)
    }

    /// Where server `name` is, if the user has set it up, as the thread last read it from the settings.
    fn program(&self, name: ServerName) -> Option<&PathBuf> {
        self.known.get(&name).and_then(Option::as_ref)
    }

    /// Notes that the server `key` names started from `program`, naming `version`, if it did, and looks at
    /// the program's folder for what it lacks (#684).
    fn started(&mut self, key: &Key, program: &Path, version: Option<String>) {
        #[expect(
            unused_results,
            reason = "a server that has started has no last problem, whichever it had"
        )]
        self.lasts.remove(key);
        let flaw = health::flaw(key.1, program);
        self.health.entry(key.1).or_default().started(version, flaw);
    }

    /// Notes that the server `key` names has problem `problem`, and logs it.
    fn trouble(&mut self, key: &Key, problem: ServerProblem) {
        let (repository, name) = key;
        let named = name.as_str();
        log::warn!("{named} for {repository} has a problem: {problem:?}");
        self.lasts.extend([(key.clone(), Last::Problem(problem))]);
    }

    fn run(mut self) {
        while let Some(message) = self.next() {
            let stops = matches!(message, Message::Stop(..));
            self.handle(message);
            if stops {
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
            self.publish();
            let Some(wait) = self.until_idle(now, idle) else {
                return self.incoming.recv().ok();
            };
            match self.incoming.recv_timeout(wait) {
                Err(RecvTimeoutError::Timeout) => {}
                received @ (Ok(_) | Err(RecvTimeoutError::Disconnected)) => return received.ok(),
            }
        }
    }

    /// Handles `message`, and writes the servers' rows if they changed. After a `Stop` the thread ends.
    fn handle(&mut self, message: Message) {
        match message {
            Message::Opened(repository, pr) => self.opened(&repository, pr),
            Message::LookUp(lookup, asker) => self.look_up(lookup, asker),
            Message::Cancel(ticket) => self.cancel(ticket),
            Message::Answered(id, hits) => self.answered(id, hits),
            Message::Indexing(key) => self.indexing(&key),
            Message::Exited(key, exit) => self.exited(&key, exit),
            Message::Troubled(key, problem) => self.troubled(&key, problem),
            Message::Reconfigured => self.reconfigured(),
            Message::Checked(name, path, outcome) => self.checked(name, &path, outcome),
            Message::Remove(repository, reply) => {
                self.remove(&repository);
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "`remove` waits for this, and fails only once its caller has gone"
                )]
                let _ = reply.send(());
            }
            Message::Stop(patience, reply) => {
                self.cancel_running();
                let all: Vec<Key> = self.servers.keys().cloned().collect();
                self.stop_servers_until(&all, Instant::now() + patience);
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the caller ran out of patience, or let go of the thread \
                              without waiting"
                )]
                let _ = reply.send(());
                return;
            }
        }
        self.publish();
    }

    /// Forgets the server `key` names if it is the one that exited, as it ended on its own, and notes how:
    /// the next lookup starts it again. A lookup it was asked fails by itself. A server Tsugiki stopped is
    /// forgotten already, and a newer one at the same key hasn't ended.
    fn exited(&mut self, key: &Key, exit: Exit) {
        let ended =
            (self.servers.get(key)).is_some_and(|server| server.indexing() == Indexing::Ended);
        if !ended {
            return;
        }
        #[expect(
            unused_results,
            reason = "the server has ended, and the watch on its index ends with the news that its index \
                      ended, which comes next"
        )]
        self.servers.remove(key);
        self.trouble(key, ServerProblem::stopped(exit));
    }

    /// Notes `problem` of the server `key` names, which runs, unless it has stopped since or has a problem
    /// that explains as much, and logs it.
    fn troubled(&mut self, key: &Key, problem: ServerProblem) {
        if !self.servers.contains_key(key) {
            return;
        }
        let (repository, name) = key;
        let logged = format!("{problem:?}");
        if self.lasts.entry(key.clone()).or_default().troubled(problem) {
            let named = name.as_str();
            log::warn!("{named} for {repository} runs with a problem: {logged}");
        }
    }

    /// The problem of the server `key` names, which runs, that may explain why it found nothing, if it has
    /// one: what its program lacks, or what it reported.
    fn misleads(&self, key: &Key) -> Option<ServerProblem> {
        let health = self.health.get(&key.1).cloned().unwrap_or_default();
        let last = self.lasts.get(key).unwrap_or(&Last::Nothing);
        health.misleads(last).cloned()
    }

    /// Reads each server's path from the settings again. Stops each server whose path has changed since,
    /// cancels a lookup that runs on one, forgets what it knew of it, and checks the new path with
    /// `--version` on a thread of its own, and its folder for what it lacks.
    fn reconfigured(&mut self) {
        for name in ServerName::ALL {
            let path = (self.sources.servers)(name);
            if self.known.get(&name) == Some(&path) {
                continue;
            }
            self.known.extend([(name, path.clone())]);
            let theirs: Vec<Key> = (self.servers.keys())
                .filter(|(_, running)| *running == name)
                .cloned()
                .collect();
            self.stop_servers(&theirs);
            self.lasts.retain(|(_, server), _| *server != name);
            let Some(path) = path else {
                #[expect(
                    unused_results,
                    reason = "a server that isn't set up has no health to show"
                )]
                self.health.remove(&name);
                continue;
            };
            self.health.extend([(name, Health::checking())]);
            let own = self.own.clone();
            #[expect(
                unused_results,
                reason = "the check ends by itself, and an owner that has gone doesn't need its word"
            )]
            thread::spawn(move || {
                let found = health::check(name, &path);
                tell(&own, Message::Checked(name, path, found));
            });
        }
    }

    /// Notes what checking the program at `path` found of server `name`, unless its path has changed since,
    /// or a server has started from it.
    fn checked(&mut self, name: ServerName, path: &Path, outcome: Result<Found, ServerProblem>) {
        if self.program(name).is_none_or(|known| known != path) {
            return;
        }
        if let Some(health) = self.health.get_mut(&name) {
            health.checked(outcome);
        }
    }

    /// Stops `repository`'s servers and forgets its PR and what happened to its servers, cancels the lookup
    /// running on one of them, and waits until the servers' own processes have ended.
    fn remove(&mut self, repository: &RepositoryName) {
        #[expect(unused_results, reason = "a Repository that has gone has no PR open")]
        self.prs.remove(repository);
        self.lasts.retain(|(owner, _), _| owner != repository);
        if self.shown.as_ref() == Some(repository) {
            self.shown = None;
        }
        let theirs: Vec<Key> = (self.servers.keys())
            .filter(|(owner, _)| owner == repository)
            .cloned()
            .collect();
        self.stop_servers_until(&theirs, Instant::now() + ENDED_WITHIN);
    }

    /// Asks the servers `keys` name to stop, as `forget_servers` says, and returns at once.
    fn stop_servers(&mut self, keys: &[Key]) {
        self.forget_servers(keys)
            .into_iter()
            .for_each(LanguageServer::stop);
    }

    /// Asks the servers `keys` name to stop, as `forget_servers` says, and waits until each has ended, or
    /// until `until`.
    fn stop_servers_until(&mut self, keys: &[Key], until: Instant) {
        let servers = self.forget_servers(keys).into_iter();
        let stopping: Vec<Stopping> = servers.map(LanguageServer::stopping).collect();
        for server in stopping {
            server.wait(until);
        }
    }

    /// Takes out the servers `keys` name, and cancels the lookup running on one of them. A lookup that hears
    /// how the index of one goes hears nothing more.
    fn forget_servers(&mut self, keys: &[Key]) -> Vec<LanguageServer> {
        let asked = self
            .running
            .take_if(|running| keys.contains(&running.server));
        asked.into_iter().for_each(Running::cancel);
        for key in keys {
            self.unwatch(key);
        }
        (keys.iter())
            .filter_map(|key| self.servers.remove(key))
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
        for key in &stale {
            let (repository, name) = key;
            let named = name.as_str();
            log::info!(
                "Tsugiki stops {named} for {repository}, which no lookup has reached for {minutes} minutes"
            );
            self.lasts.extend([(key.clone(), Last::Idle)]);
        }
        self.stop_servers(&stale);
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
        self.shown = Some(repository.clone());
        if self
            .prs
            .get(repository)
            .is_some_and(|known| known.at == pr.at)
        {
            return;
        }
        self.prs.extend([(repository.clone(), pr.clone())]);
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
        self.stop_servers(&stale);
    }

    /// Starts the server `key` names for `pr`, if the user has set it up.
    fn start(&mut self, key: &Key, pr: &Pr) {
        let (repository, name) = key;
        let Some(program) = self.program(*name).cloned() else {
            return;
        };
        let replaces = (self.servers.get(key)).is_some_and(|server| *server.at() != pr.at);
        let checkout = (&self.sources.checkout, &self.own);
        let started = ready(&mut self.servers, checkout, (key, pr), (&program, &|_| {}));
        match started.map(|server| server.version()) {
            Ok(version) => self.started(key, &program, version),
            Err(NotReady::Problem(problem)) => self.trouble(key, problem),
            Err(NotReady::Failed(problem)) => {
                let named = name.as_str();
                let why = problem.message;
                log::warn!("Tsugiki couldn't start {named} for {repository}: {why}");
            }
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
            #[expect(unused_results, reason = "it is `asker`'s ticket, which `asker` holds")]
            self.cancelled.remove(at);
            asker.answer(Ok(Answer::Cancelled));
            return;
        }
        self.cancel_running();
        self.watching = None;
        self.shown = Some(lookup.repository.clone());
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
        let Some(program) = self.program(*name).cloned() else {
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
        let server = match ready(&mut self.servers, checkout, (key, &pr), start) {
            Ok(server) => server,
            Err(NotReady::Problem(problem)) => {
                self.trouble(key, problem.clone());
                let server = *name;
                return Ok(Sent::Answered(Answer::Problem { server, problem }));
            }
            Err(NotReady::Failed(failed)) => return Err(failed),
        };
        if starts {
            // As `started` does, field by field, as `server` borrows `servers`.
            #[expect(
                unused_results,
                reason = "a server that has started has no last problem, whichever it had"
            )]
            self.lasts.remove(key);
            let flaw = health::flaw(*name, &program);
            (self.health.entry(*name).or_default()).started(server.version(), flaw);
            // The user may have asked again, or cancelled, while the server started.
            self.queued.extend(self.incoming.try_iter());
            let newer = |message: &Message| match message {
                Message::LookUp(..) => true,
                Message::Cancel(ticket) => *ticket == asker.ticket,
                Message::Opened(..)
                | Message::Answered(..)
                | Message::Indexing(..)
                | Message::Exited(..)
                | Message::Troubled(..)
                | Message::Reconfigured
                | Message::Checked(..)
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
        #[expect(
            unused_results,
            reason = "it ends once the server answers, the request is cancelled or the server ends"
        )]
        thread::spawn(move || {
            let answer = pending.wait().map(|definitions| hits.of(definitions));
            tell(&own, Message::Answered(id, answer));
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
            (Err(RequestError::Exited), Asked::First { asker, .. }) => {
                asker.answer(self.ended(&server));
                return;
            }
            (Err(cause), Asked::First { asker, .. }) => {
                #[expect(
                    unused_results,
                    reason = "dropping a server that failed stops it, and the next lookup starts it afresh"
                )]
                self.servers.remove(&server);
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
        // A clangd that runs with a problem says it in place of nothing found (#684).
        let problem = self.misleads(&server).filter(|_| hits.is_empty());
        match asked {
            Asked::Again(listener) => {
                // Nothing found again leaves the answer, which says the problem.
                (listener.index)(if problem.is_some() {
                    Index::Stopped
                } else {
                    Index::Built { hits }
                });
            }
            Asked::First {
                asker,
                request,
                indexing,
            } => {
                asker.answer(Ok(match problem {
                    Some(problem) => Answer::Problem {
                        server: server.1,
                        problem,
                    },
                    None => Answer::Found { hits },
                }));
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

    /// The answer to a lookup whose server `key` ended before it answered, which the next lookup starts
    /// again: how it stopped, once the client has heard, or else why the lookup failed. The client hears
    /// within moments, so this waits up to [`EXIT_NEWS`] for it, unless the thread has heard already.
    fn ended(&mut self, key: &Key) -> Result<Answer, Error> {
        let exit = (self.servers.remove(key)).and_then(|server| server.exit_within(EXIT_NEWS));
        if let Some(exit) = exit {
            self.trouble(key, ServerProblem::stopped(exit));
        }
        let (_, name) = key;
        match self.lasts.get(key) {
            Some(Last::Problem(problem @ ServerProblem::Stopped { .. })) => Ok(Answer::Problem {
                server: *name,
                problem: problem.clone(),
            }),
            _ => {
                let named = name.as_str();
                let cause = RequestError::Exited;
                Err(failed(&format!("{named} couldn't answer: {cause}")))
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
    /// is built, it asks the lookup again, and once the server has ended, it says so. A server that reports
    /// its index no longer has the problem of one that doesn't.
    fn indexing(&mut self, key: &Key) {
        // A clangd that reports its index after its time is indexing all the same.
        if let Indexing::Building(_) | Indexing::Built = self.indexing_of(key)
            && let Some(last) = self.lasts.get_mut(key)
        {
            last.indexed();
        }
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
) -> Result<&'a mut LanguageServer, NotReady> {
    let (repository, name) = key;
    let server = match servers.remove(key) {
        Some(running) if *running.at() == pr.at => running,
        old => {
            let Some(checkout) = checkout(repository).filter(|checkout| checkout.at == pr.at)
            else {
                if let Some(old) = old {
                    servers.extend([(key.clone(), old)]);
                }
                let elsewhere = format!(
                    "Tsugiki has no copy of this PR's files for {} to read yet.",
                    name.as_str()
                );
                return Err(Error::new(ErrorKind::NotFound, &elsewhere).into());
            };
            waiting(Waiting::Starting { server: *name });
            // Asked first, the old server can end while the new one starts.
            if let Some(old) = old {
                old.stop();
            }
            let changed = pr.changed_for(*name);
            LanguageServer::start(*name, program, (checkout, &changed), news(own, key))?
        }
    };
    Ok(servers.entry(key.clone()).insert_entry(server).into_mut())
}

/// Tells the thread at `own` the news of the server `key` names: its index has moved on, it exited, or it
/// runs with a problem.
fn news(own: &Sender<Message>, key: &Key) -> OnNews {
    let (own, key) = (own.clone(), key.clone());
    Arc::new(move |news| {
        let message = match news {
            News::Indexing => Message::Indexing(key.clone()),
            News::Exited(exit) => Message::Exited(key.clone(), exit),
            News::Troubled(problem) => Message::Troubled(key.clone(), problem),
        };
        tell(&own, message);
    })
}

/// Sends the servers' thread `message`.
fn tell(own: &Sender<Message>, message: Message) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "the thread ends once it has stopped, and then nothing is left to do with a message"
    )]
    let _ = own.send(message);
}

/// `path` without its `.` and `..` parts, as a server names the files it read.
fn plain(path: &Path) -> PathBuf {
    let mut plain = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            #[expect(
                unused_results,
                reason = "a `..` above the first part has nothing to take away, and is left out"
            )]
            plain.pop();
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
