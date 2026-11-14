//! Where a name in the open PR's code is defined, which the Definition peek and Go to definition ask a
//! language server (ADR 0020). ty answers in Python.
//!
//! Each Repository gets its own ty, which starts when a PR that changes a Python file opens and reads that
//! Repository's **Lookup checkout**: the PR's base on disk, with the files the PR adds. Each Python file the
//! PR changes is open on ty with its text at the head. A lookup from a removed line first gives those files
//! their text at the base, and the next lookup from another line puts the head's back. A PR at other commits
//! gets a new ty, as its checkout has moved.
//!
//! One thread owns the servers, and each lookup is a message to it, so the window never waits on ty.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use specta::Type;
use tsugiki_lsp::Uri;

use crate::diff::DiffFile;
use crate::error::{Error, ErrorKind};
use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;
use code::HitFiles;
pub use code::{Code, HitFile};
use ty::{Failure, Ty};

mod code;
#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
mod ty;

/// The longest ty gets to start, or to answer a lookup. ty answers in well under a second once it runs, but
/// its first lookup in a big Repository such as CPython reads many files.
const PATIENCE: Duration = Duration::from_secs(30);

/// The commit a line of the diff is at: the PR's base for a removed line, its head for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffEnd {
    Base,
    Head,
}

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
}

/// Where a Repository's Lookup checkout is, once it has stopped moving.
pub type FindCheckout = Box<dyn Fn(&RepositoryName) -> Option<LookupCheckout> + Send>;

/// What the servers' thread asks the rest of Tsugiki.
pub struct Sources {
    pub checkout: FindCheckout,
    /// Where ty is, if the user has set it up.
    pub ty: Box<dyn Fn() -> Option<PathBuf> + Send>,
}

/// The handle the rest of Tsugiki asks the servers' thread through.
#[derive(Clone)]
pub struct Definitions {
    messages: Sender<Message>,
    /// The files [`Definitions::code`] reads.
    files: Arc<HitFiles>,
}

impl Definitions {
    /// Starts the thread that owns the servers. It starts none until a PR asks for one.
    pub fn start(sources: Sources) -> Self {
        let (messages, incoming) = mpsc::channel();
        let owner = Owner::new(sources);
        let files = Arc::clone(&owner.files);
        let _owner = thread::spawn(move || owner.run(&incoming));
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

    /// Where `lookup`'s name is defined. It waits for ty, and for ty to start if it hasn't, so the window
    /// asks it from a thread of its own.
    pub fn look_up(&self, lookup: Lookup) -> Result<Answer, Error> {
        let (reply, answer) = mpsc::channel();
        self.send(Message::LookUp(lookup, reply));
        let gone = Error::new(ErrorKind::Internal, "Tsugiki's lookups have stopped.");
        answer.recv().unwrap_or(Err(gone))
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
        let _unheard = self.messages.send(message);
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
    LookUp(Lookup, Sender<Result<Answer, Error>>),
    Stop(Duration, Sender<()>),
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
    /// How long a lookup waits for ty's answer: [`PATIENCE`], and less in a test.
    patience: Duration,
    /// The files the peek reads, among them those outside the Repository that a hit has named.
    files: Arc<HitFiles>,
}

impl Owner {
    fn new(sources: Sources) -> Self {
        Self {
            sources,
            prs: HashMap::new(),
            servers: HashMap::new(),
            patience: PATIENCE,
            files: Arc::default(),
        }
    }

    fn run(mut self, incoming: &Receiver<Message>) {
        for message in incoming {
            match message {
                Message::Opened(repository, pr) => self.opened(&repository, pr),
                Message::LookUp(lookup, reply) => {
                    // The window may have stopped waiting.
                    let _unheard = reply.send(self.look_up(&lookup));
                }
                Message::Stop(patience, reply) => {
                    self.stop(patience);
                    let _unheard = reply.send(());
                    return;
                }
            }
        }
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
        if let Err(problem) = self.ready(repository, &pr) {
            log::warn!(
                "Tsugiki couldn't start ty for {repository}: {}",
                problem.message
            );
        }
    }

    fn look_up(&mut self, lookup: &Lookup) -> Result<Answer, Error> {
        if !is_python(&lookup.path) {
            return Ok(Answer::NoServer);
        }
        let repository = &lookup.repository;
        let unknown = Pr {
            at: lookup.at.clone(),
            changed: Vec::new(),
        };
        let pr = (self.prs.get(repository))
            .filter(|pr| pr.at == lookup.at)
            .cloned()
            .unwrap_or(unknown);
        let patience = self.patience;
        let Some(ty) = self.ready(repository, &pr)? else {
            return Ok(Answer::NotSetUp);
        };
        match ty.look_up(lookup, patience) {
            Ok(hits) => {
                for hit in &hits {
                    if let Place::Outside { path } = &hit.place {
                        self.files.name(PathBuf::from(path));
                    }
                }
                Ok(Answer::Found { hits })
            }
            Err(Failure::Slow(problem)) => Err(problem),
            Err(Failure::Broken(problem)) => {
                // A ty that failed starts afresh on the next lookup.
                let _failed = self.servers.remove(repository);
                Err(problem)
            }
        }
    }

    /// The ty that reads `pr`, started if none does, or none if the user hasn't set ty up. A ty that reads
    /// another PR's checkout stops once the checkout is at `pr`, and goes on while it isn't.
    fn ready(&mut self, repository: &RepositoryName, pr: &Pr) -> Result<Option<&mut Ty>, Error> {
        let Some(program) = (self.sources.ty)() else {
            return Ok(None);
        };
        if self
            .servers
            .get(repository)
            .is_some_and(|ty| *ty.at() == pr.at)
        {
            return Ok(self.servers.get_mut(repository));
        }
        let elsewhere = Error::new(
            ErrorKind::NotFound,
            "Tsugiki has no copy of this PR's files for ty to read yet.",
        );
        let checkout = (self.sources.checkout)(repository)
            .filter(|checkout| checkout.at == pr.at)
            .ok_or(elsewhere)?;
        if let Some(old) = self.servers.remove(repository) {
            let _stopping = old.shutdown();
        }
        let ty = Ty::start(&program, checkout, &pr.changed)?;
        Ok(Some(self.servers.entry(repository.clone()).or_insert(ty)))
    }
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
