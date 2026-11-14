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
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;
use tsugiki_lsp::{Document, Event, Init, Language, Launch, Location, Position, Server, Uri};

use crate::diff::{self, DiffFile};
use crate::error::{Error, ErrorKind};
use crate::github::CommitSha;
use crate::mirror::{LookupAt, LookupCheckout};
use crate::repository::RepositoryName;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

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
}

impl Definitions {
    /// Starts the thread that owns the servers. It starts none until a PR asks for one.
    pub fn start(sources: Sources) -> Self {
        let (messages, incoming) = mpsc::channel();
        let _owner = thread::spawn(move || Owner::new(sources).run(&incoming));
        Self { messages }
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
}

impl Owner {
    fn new(sources: Sources) -> Self {
        Self {
            sources,
            prs: HashMap::new(),
            servers: HashMap::new(),
            patience: PATIENCE,
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
            // Its process ends once it agrees, which `Event::Exited` says.
            let _agreed = ty.server.shutdown();
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
            Ok(answer) => Ok(answer),
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
            .is_some_and(|ty| ty.at == pr.at)
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
            // Dropping the server once it has agreed to stop lets it end on its own.
            let _stopping = old.server.shutdown();
        }
        let ty = Ty::start(&program, checkout, &pr.changed)?;
        Ok(Some(self.servers.entry(repository.clone()).or_insert(ty)))
    }
}

/// Why ty gave no answer to a lookup.
enum Failure {
    /// It is still working on it, as on its first lookup in a big Repository, and goes on running.
    Slow(Error),
    /// It answered with an error, or ended, and starts afresh on the next lookup.
    Broken(Error),
}

/// One ty, which reads a Lookup checkout, with the PR's changed Python files open on it.
struct Ty {
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
    fn start(program: &Path, checkout: LookupCheckout, changed: &[String]) -> Result<Self, Error> {
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

    /// Asks ty, and waits up to `patience` for its answer. A lookup ty hasn't answered by then is cancelled.
    fn look_up(&mut self, lookup: &Lookup, patience: Duration) -> Result<Answer, Failure> {
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
    fn wait_for_exit(&self, until: Instant) {
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
