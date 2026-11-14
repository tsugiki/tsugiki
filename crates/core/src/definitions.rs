//! Where a name in the open PR's code is defined, which the Definition peek and Go to definition ask a
//! language server (ADR 0020). ty answers in Python.
//!
//! Each Repository gets its own ty, which starts when a PR that changes a Python file opens and reads that
//! Repository's **Lookup checkout**: the PR's base on disk, with the files the PR adds. Each Python file the
//! PR changes is open on ty with its text at the head. A lookup from a removed line first gives those files
//! their text at the base, and the next lookup from another line puts the head's back. A PR at other commits
//! gets a new ty, as its checkout has moved.
//!
//! One thread owns the servers, and each lookup is a message to it, so the window never waits on ty. The
//! thread sends ty the request and goes on, and a thread of the lookup's own waits for ty's answer and hands
//! it back as a message. A newer lookup, or a cancel, cancels the one running.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tsugiki_lsp::{Canceller, RequestError, Uri};

pub use crate::diff::DiffEnd;
use crate::diff::DiffFile;
use crate::error::{Error, ErrorKind};
use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;
use code::HitFiles;
pub use code::{Code, HitFile};
use ty::Ty;

mod code;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod ty;

/// The longest ty gets to start. A lookup has no time limit, as the user can cancel it.
const PATIENCE: Duration = Duration::from_secs(30);

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
    /// LSP does, so the column goes to ty as it is.
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
    /// A file outside the Repository, such as a stub of the standard library that ty keeps, by its path on
    /// disk.
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
    /// The line's text, as ty read it.
    pub text: String,
}

/// What a lookup found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Answer {
    /// The places ty named, in its order, which may be none.
    Found { hits: Vec<Hit> },
    /// No ty is set up, so Tsugiki can't look up a name in Python.
    NotSetUp,
    /// Tsugiki has no server for the file's language yet.
    NoServer,
    /// A newer lookup, or the user, cancelled it before the server answered.
    Cancelled,
}

/// A language server Tsugiki talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ServerName {
    Ty,
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

/// The number the window gives a lookup, which a cancel names. The window may cancel a lookup before the
/// core has taken it, or after it has answered, so a cancel names the lookup it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
pub struct Ticket(pub u32);

/// Who hears what a lookup waits for, as that changes.
pub type OnWaiting = Box<dyn Fn(Waiting) + Send>;

/// Where a Repository's Lookup checkout is, once it has stopped moving.
pub type FindCheckout = Box<dyn Fn(&RepositoryName) -> Option<LookupCheckout> + Send>;

/// What the servers' thread asks the rest of Tsugiki.
pub struct Sources {
    pub checkout: FindCheckout,
    /// Where ty is, if the user has set it up.
    pub ty: Box<dyn Fn() -> Option<PathBuf> + Send>,
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

    /// Starts ty for the PR whose diff at `at` changes `files`, if one of them is in Python, and returns at
    /// once. ty starts once the Lookup checkout is at `at`.
    pub fn opened(&self, repository: RepositoryName, at: LookupAt, files: &[DiffFile]) {
        let changed = python_files(files);
        if !changed.is_empty() {
            self.send(Message::Opened(repository, Pr { at, changed }));
        }
    }

    /// Where `lookup`'s name is defined. It cancels the lookup running, if one is, and waits for ty, and for
    /// ty to start if it hasn't, so the window asks it from a thread of its own. `waiting` hears what it
    /// waits for as that changes, and a cancel that names `ticket` cancels it.
    pub fn look_up(
        &self,
        lookup: Lookup,
        ticket: Ticket,
        waiting: OnWaiting,
    ) -> Result<Answer, Error> {
        let (reply, answer) = mpsc::channel();
        let asker = Asker {
            ticket,
            reply,
            waiting,
        };
        self.send(Message::LookUp(lookup, asker));
        let gone = Error::new(ErrorKind::Internal, "Tsugiki's lookups have stopped.");
        answer.recv().unwrap_or(Err(gone))
    }

    /// Cancels the lookup with `ticket`, whose answer is then [`Answer::Cancelled`]: the one running, or the
    /// next one the thread takes. A cancel for a lookup that has answered does nothing.
    pub fn cancel(&self, ticket: Ticket) {
        self.send(Message::Cancel(ticket));
    }

    /// Asks each ty to stop, and waits up to `patience` for them to end. The thread then ends, and a later
    /// lookup says so.
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

/// The paths at the head of the Python files in `files`. A path with a line break can't be read with
/// `git cat-file --batch`, which reads a name a line.
fn python_files(files: &[DiffFile]) -> Vec<String> {
    (files.iter())
        .map(|file| &file.path)
        .filter(|path| is_python(path) && !path.contains('\n'))
        .cloned()
        .collect()
}

fn is_python(path: &str) -> bool {
    path.ends_with(".py") || path.ends_with(".pyi")
}

enum Message {
    Opened(RepositoryName, Pr),
    LookUp(Lookup, Asker),
    Cancel(Ticket),
    /// ty's answer to the lookup numbered so, as the places it names, which a thread of the lookup's own
    /// hands back.
    Answered(u64, Result<Vec<Hit>, RequestError>),
    Stop(Duration, Sender<()>),
}

/// Who asked a lookup: the lookup's ticket, where its answer goes, and who hears what it waits for.
struct Asker {
    ticket: Ticket,
    reply: Sender<Result<Answer, Error>>,
    waiting: OnWaiting,
}

impl Asker {
    fn answer(&self, answer: Result<Answer, Error>) {
        // The window may have stopped waiting.
        let _unheard = self.reply.send(answer);
    }
}

/// What became of a lookup the servers' thread took.
enum Sent {
    /// It has its answer, and ty was asked nothing.
    Answered(Answer),
    /// ty was asked, and its answer comes back with this number. The canceller cancels it.
    Asked(u64, Canceller),
}

/// The lookup ty works on.
struct Running {
    /// The number its answer comes back with.
    id: u64,
    asker: Asker,
    repository: RepositoryName,
    canceller: Canceller,
}

/// A PR on screen: its diff's two ends, and the paths at the head of the Python files it changes.
#[derive(Debug, Clone)]
struct Pr {
    at: LookupAt,
    changed: Vec<String>,
}

/// The servers' thread.
struct Owner {
    sources: Sources,
    /// The last PR each Repository opened that changes a Python file.
    prs: HashMap<RepositoryName, Pr>,
    servers: HashMap<RepositoryName, Ty>,
    /// The files the peek reads, among them those outside the Repository that a hit has named.
    files: Arc<HitFiles>,
    /// The lookup ty works on, if one is.
    running: Option<Running>,
    /// The ticket of a cancel that came before its lookup, which cancels that lookup if it comes next.
    cancelled: Option<Ticket>,
    /// How many lookups ty has been asked.
    asked: u64,
    /// The thread's own messages: those it takes, and the way the threads that wait for ty hand back its
    /// answers. Holding a sender keeps the channel open, so the thread ends only on a stop.
    own: Sender<Message>,
    incoming: Receiver<Message>,
    /// Messages taken early, to see whether a newer lookup came while ty started, and not handled yet.
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
            cancelled: None,
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

    /// The next message to handle, which waits for one.
    fn next(&mut self) -> Option<Message> {
        (self.queued.pop_front()).or_else(|| self.incoming.recv().ok())
    }

    /// Handles `message`, and says whether the thread goes on.
    fn handle(&mut self, message: Message) -> bool {
        match message {
            Message::Opened(repository, pr) => self.opened(&repository, pr),
            Message::LookUp(lookup, asker) => self.look_up(lookup, asker),
            Message::Cancel(ticket) => self.cancel(ticket),
            Message::Answered(id, hits) => self.answered(id, hits),
            Message::Stop(patience, reply) => {
                self.cancel_running();
                self.stop(patience);
                let _unheard = reply.send(());
                return false;
            }
        }
        true
    }

    /// Asks each ty to stop, then waits up to `patience` in all for them to end.
    fn stop(&mut self, patience: Duration) {
        let until = Instant::now() + patience;
        let stopping: Vec<Ty> = self.servers.drain().map(|(_, ty)| ty).collect();
        for ty in &stopping {
            let _agreed = ty.shutdown();
        }
        for ty in stopping {
            ty.wait_for_exit(until);
        }
    }

    /// Starts ty for `pr`, unless `pr` is the Repository's PR already. The Files tab reads its PR again on
    /// each change, and a ty that failed to start, or a checkout that failed to move, waits for a lookup to
    /// try again.
    fn opened(&mut self, repository: &RepositoryName, pr: Pr) {
        if self
            .prs
            .get(repository)
            .is_some_and(|known| known.at == pr.at)
        {
            return;
        }
        let _old = self.prs.insert(repository.clone(), pr.clone());
        let Some(program) = (self.sources.ty)() else {
            return;
        };
        let replaces = (self.servers.get(repository)).is_some_and(|ty| *ty.at() != pr.at);
        let checkout = &self.sources.checkout;
        let started = ready(
            &mut self.servers,
            checkout,
            (repository, &pr),
            (&program, &|_| {}),
        );
        match started.map(|_ty| ()) {
            // The ty that the lookup running asked has stopped.
            Ok(()) if replaces && self.asks(repository) => self.cancel_running(),
            Ok(()) => {}
            Err(problem) => log::warn!(
                "Tsugiki couldn't start ty for {repository}: {}",
                problem.message
            ),
        }
    }

    /// Cancels the lookup running, if one is, and sends ty the request for `lookup`, unless a cancel for it
    /// came first, or a newer lookup or its cancel came while ty started. Its answer comes back as
    /// [`Message::Answered`].
    fn look_up(&mut self, lookup: Lookup, asker: Asker) {
        self.cancel_running();
        if self.cancelled.take() == Some(asker.ticket) {
            asker.answer(Ok(Answer::Cancelled));
            return;
        }
        match self.send(&lookup, &asker) {
            Ok(Sent::Asked(id, canceller)) => {
                self.running = Some(Running {
                    id,
                    asker,
                    repository: lookup.repository,
                    canceller,
                });
            }
            Ok(Sent::Answered(answer)) => asker.answer(Ok(answer)),
            Err(problem) => asker.answer(Err(problem)),
        }
    }

    /// Sends ty the request for `lookup`, starting ty if it has to, and tells `asker` what it waits for.
    fn send(&mut self, lookup: &Lookup, asker: &Asker) -> Result<Sent, Error> {
        if !is_python(&lookup.path) {
            return Ok(Sent::Answered(Answer::NoServer));
        }
        let Some(program) = (self.sources.ty)() else {
            return Ok(Sent::Answered(Answer::NotSetUp));
        };
        let repository = &lookup.repository;
        let unknown = Pr {
            at: lookup.at.clone(),
            changed: Vec::new(),
        };
        let pr = (self.prs.get(repository))
            .filter(|pr| pr.at == lookup.at)
            .cloned()
            .unwrap_or(unknown);
        let server = ServerName::Ty;
        let starts = !self.runs(repository, &pr.at);
        let checkout = &self.sources.checkout;
        let waiting: &dyn Fn(Waiting) = &asker.waiting;
        let start = (program.as_path(), waiting);
        let ty = ready(&mut self.servers, checkout, (repository, &pr), start)?;
        if starts {
            // The user may have asked again, or cancelled, while ty started.
            self.queued.extend(self.incoming.try_iter());
            let newer = |message: &Message| match message {
                Message::LookUp(..) => true,
                Message::Cancel(ticket) => *ticket == asker.ticket,
                Message::Opened(..) | Message::Answered(..) | Message::Stop(..) => false,
            };
            if self.queued.iter().any(newer) {
                return Ok(Sent::Answered(Answer::Cancelled));
            }
        }
        (asker.waiting)(Waiting::Asking { server });
        let (pending, hits) = ty.ask(lookup)?;
        self.asked += 1;
        let id = self.asked;
        let canceller = pending.canceller();
        let own = self.own.clone();
        let _waits = thread::spawn(move || {
            let answer = pending.wait().map(|definitions| hits.of(definitions));
            // The thread ends once it has stopped.
            let _unheard = own.send(Message::Answered(id, answer));
        });
        Ok(Sent::Asked(id, canceller))
    }

    /// Hands the asker of the lookup numbered `id` ty's answer, unless it was cancelled since.
    fn answered(&mut self, id: u64, hits: Result<Vec<Hit>, RequestError>) {
        let Some(running) = self.running.take_if(|running| running.id == id) else {
            return;
        };
        let answer = match hits {
            Ok(hits) => {
                // The peek may read a file outside the Repository once a hit names it.
                for hit in &hits {
                    if let Place::Outside { path } = &hit.place {
                        self.files.name(PathBuf::from(path));
                    }
                }
                Ok(Answer::Found { hits })
            }
            Err(cause) => {
                // A ty that failed starts afresh on the next lookup.
                let _failed = self.servers.remove(&running.repository);
                Err(failed(&format!("ty couldn't answer: {cause}")))
            }
        };
        running.asker.answer(answer);
    }

    /// Cancels the lookup with `ticket`: the one running, or else the next one if it is that.
    fn cancel(&mut self, ticket: Ticket) {
        if (self.running.as_ref()).is_some_and(|running| running.asker.ticket == ticket) {
            self.cancel_running();
        } else {
            self.cancelled = Some(ticket);
        }
    }

    /// Cancels the lookup running, if one is: ty stops working on it, and its asker hears it was cancelled.
    fn cancel_running(&mut self) {
        if let Some(running) = self.running.take() {
            running.canceller.cancel();
            running.asker.answer(Ok(Answer::Cancelled));
        }
    }

    /// Whether the lookup running asked `repository`'s ty.
    fn asks(&self, repository: &RepositoryName) -> bool {
        (self.running.as_ref()).is_some_and(|running| running.repository == *repository)
    }

    /// Whether a ty reads `at`'s checkout for `repository`.
    fn runs(&self, repository: &RepositoryName, at: &LookupAt) -> bool {
        (self.servers.get(repository)).is_some_and(|ty| ty.at() == at)
    }
}

/// The ty in `servers` that reads `pr` for `repository`, started at `program` if none does, which `waiting`
/// hears. A ty that reads another PR's checkout stops once the checkout is at `pr`, and goes on while it
/// isn't.
fn ready<'a>(
    servers: &'a mut HashMap<RepositoryName, Ty>,
    checkout: &FindCheckout,
    (repository, pr): (&RepositoryName, &Pr),
    (program, waiting): (&Path, &dyn Fn(Waiting)),
) -> Result<&'a mut Ty, Error> {
    let entry = match servers.entry(repository.clone()) {
        Entry::Occupied(running) if *running.get().at() == pr.at => return Ok(running.into_mut()),
        entry @ (Entry::Occupied(_) | Entry::Vacant(_)) => entry,
    };
    let elsewhere = Error::new(
        ErrorKind::NotFound,
        "Tsugiki has no copy of this PR's files for ty to read yet.",
    );
    let checkout = checkout(repository)
        .filter(|checkout| checkout.at == pr.at)
        .ok_or(elsewhere)?;
    waiting(Waiting::Starting {
        server: ServerName::Ty,
    });
    if let Entry::Occupied(old) = &entry {
        let _stopping = old.get().shutdown();
    }
    let ty = Ty::start(program, checkout, &pr.changed)?;
    Ok(entry.insert_entry(ty).into_mut())
}

/// `path` without its `.` and `..` parts, as ty names the files it read.
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
