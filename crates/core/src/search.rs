//! Text search over the open PR's code at its head (ADR 0020), a separate action from Go to definition.
//!
//! It runs on ripgrep's crates. It reads the **Lookup checkout** for the files the PR doesn't change and its
//! added files, and the PR's head from the **Mirror** for the files it changes, and leaves out the files it
//! deletes. It needs nothing but the Mirror and the checkout, so it works offline at any commit the Mirror
//! holds. A query that arrives while the checkout moves waits for the move: `git grep` could answer sooner,
//! but its idea of a regex and of a word differ from ripgrep's, so a query would match differently by timing.
//!
//! One thread owns what the window reads, a [`SearchState`]. Each query runs on threads of its own, which
//! send it what they find, and a newer query cancels the one running.

mod engine;
mod tree;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::latest::{self, LatestReader, LatestWriter};
use crate::mirror::{LookupCheckout, MirrorRequests};
use crate::repository::RepositoryName;

pub use engine::LONGEST_LINE;

/// The most matches a search hands the window. It stops there, and says so.
pub const MOST_MATCHES: u32 = 5_000;

/// How long a query waits for the Lookup checkout before it says that it waits. A checkout that isn't
/// moving answers at once, so a query says nothing of a wait nobody would see.
const WAIT_UNSAID: Duration = Duration::from_millis(100);

/// What to look for, and how, with VS Code's options. By default a query is a literal that matches
/// whatever the case, inside a longer word too.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Query {
    pub text: String,
    pub case: Case,
    pub words: Words,
    pub syntax: Syntax,
}

/// Whether a match keeps the query's case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Case {
    Ignored,
    Matched,
}

/// Whether a match can sit inside a longer word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Words {
    /// Anywhere, as in `spam` inside `spammer`.
    Within,
    /// Only a whole word, with no letter, digit or `_` on either side.
    Whole,
}

/// How a query's text reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Syntax {
    /// Each character stands for itself.
    Literal,
    /// A regex in the Rust regex crate's syntax.
    Regex,
}

/// Why a query can't run: a regex that doesn't parse, in the regex crate's words, or no text at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct BadQuery {
    pub message: String,
}

/// The lines of one file that hold a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct FileHits {
    /// The path from the top of the Repository, with `/` between folders.
    pub path: String,
    /// Whether the `.gitattributes` in the Lookup checkout mark it `linguist-generated`, as the Files tab
    /// marks a Generated file. Its matches count as any other's.
    pub generated: bool,
    pub lines: Vec<LineHit>,
}

/// A line that holds a match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct LineHit {
    /// Its number, from 1.
    pub line: u32,
    /// Its text with no line ending: all of it, or [`LONGEST_LINE`] characters around its first match.
    pub text: String,
    pub cut: Cut,
    /// Where each match is in `text`, in order. A match across the edge of a cut line is cut with it.
    pub ranges: Vec<MatchRange>,
}

/// Which ends of a long line its text leaves out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Cut {
    /// The text is the whole line.
    Neither,
    Start,
    End,
    Both,
}

impl LineHit {
    fn matches(&self) -> u32 {
        u32::try_from(self.ranges.len()).unwrap_or(u32::MAX)
    }
}

/// From `start` to `end` in a line's text, in UTF-16 code units, as the webview counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct MatchRange {
    pub start: u32,
    pub end: u32,
}

/// Which query a [`SearchState`] answers, as [`TextSearch::search`] numbered it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct SearchId(u32);

/// What the window shows of the latest query.
#[derive(Debug, Clone, Default)]
pub enum SearchState {
    /// No query has run since launch.
    #[default]
    Idle,
    /// The query waits for the Lookup checkout to move to the PR.
    Waiting { id: SearchId },
    /// No move has put the Repository's Lookup checkout anywhere, the last one failed, or the Mirror worker
    /// has stopped, so there is nothing to search.
    NoCheckout { id: SearchId },
    /// git couldn't list or read the PR's files, which the log says more of.
    Failed { id: SearchId },
    /// What the query found so far, or in all.
    Results(Results),
}

/// What a query found.
#[derive(Debug, Clone)]
pub struct Results {
    pub id: SearchId,
    /// The files that hold a match, by path. Each is shared, so a copy of the state copies no line.
    pub files: Vec<Arc<FileHits>>,
    /// The matches in `files`.
    pub matches: u32,
    pub end: End,
}

impl Results {
    /// Query `id`'s results before it has found anything.
    fn nothing_yet(id: SearchId) -> Self {
        Self {
            id,
            files: Vec::new(),
            matches: 0,
            end: End::Running,
        }
    }

    /// Adds `batch`, each file in its place by path.
    fn add(&mut self, batch: Vec<FileHits>) {
        for hits in batch {
            let count: u32 = hits.lines.iter().map(LineHit::matches).sum();
            self.matches = self.matches.saturating_add(count);
            let at = (self.files).partition_point(|file| file.path.as_str() < hits.path.as_str());
            self.files.insert(at, Arc::new(hits));
        }
    }
}

/// Whether a search has ended, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum End {
    /// It still reads files, so more may come.
    Running,
    /// It read every file.
    Done,
    /// It found [`MOST_MATCHES`] matches and stopped there, so some files may hold more.
    Stopped,
}

impl From<tree::Outcome> for End {
    fn from(outcome: tree::Outcome) -> Self {
        match outcome {
            tree::Outcome::Done => Self::Done,
            tree::Outcome::Stopped => Self::Stopped,
        }
    }
}

/// The text search's thread, and what it shares with the window. The thread ends once this has gone and
/// the queries it ran have ended.
pub struct TextSearch {
    messages: Sender<Message>,
    state: LatestReader<SearchState>,
    numbered: AtomicU64,
}

impl TextSearch {
    /// Starts the thread, which asks `requests` where each Lookup checkout is. It calls `announce` each time
    /// the state may have changed.
    pub fn start(requests: MirrorRequests, announce: Box<dyn Fn() + Send>) -> Self {
        let (messages, incoming) = mpsc::channel();
        let (writer, state) = latest::channel(SearchState::Idle);
        let owner = Owner {
            requests,
            announce,
            writer,
            state: SearchState::Idle,
            current: None,
        };
        let _ = thread::spawn(move || owner.run(&incoming));
        Self {
            messages,
            state,
            numbered: AtomicU64::new(0),
        }
    }

    /// Searches `repository`'s open PR for `query`, in place of the query running, and gives the number
    /// the state answers it with. A query that can't run returns why, and the one running goes on.
    pub fn search(&self, repository: RepositoryName, query: &Query) -> Result<SearchId, BadQuery> {
        let matcher = engine::Matcher::new(query)?;
        let numbered = (self.numbered.fetch_add(1, Ordering::Relaxed)).saturating_add(1);
        let id = SearchId(u32::try_from(numbered).unwrap_or(u32::MAX));
        let asked = Asked {
            id,
            repository,
            matcher,
        };
        // The thread ends only once this has gone, unless it panicked, and then no query can run.
        let _unheard = (self.messages).send(Message::Search(asked, self.messages.clone()));
        Ok(id)
    }

    /// What the latest query found, or why it found nothing.
    pub fn state(&self) -> SearchState {
        self.state.get()
    }
}

/// What reaches the text search's thread: a query from the window, with a sender for the threads that run
/// it, and word from those threads.
enum Message {
    Search(Asked, Sender<Message>),
    Word(SearchId, Word),
}

/// A query the window asked for, numbered, in the Repository whose PR is open.
struct Asked {
    id: SearchId,
    repository: RepositoryName,
    matcher: engine::Matcher,
}

/// What a query's threads say of it.
enum Word {
    Waiting,
    Started,
    Found(Vec<FileHits>),
    Ended(Ending),
}

/// How a search ended.
#[derive(Debug, Clone, Copy)]
enum Ending {
    Finished(tree::Outcome),
    NoCheckout,
    Failed,
}

/// The query running, and how to cancel it.
struct Current {
    id: SearchId,
    cancelled: Arc<AtomicBool>,
}

/// The one writer of the state.
struct Owner {
    requests: MirrorRequests,
    announce: Box<dyn Fn() + Send>,
    writer: LatestWriter<SearchState>,
    state: SearchState,
    current: Option<Current>,
}

impl Owner {
    /// Takes messages until no sender is left. All those waiting go in before the window hears, so a
    /// burst of files reaches it as one batch.
    fn run(mut self, incoming: &Receiver<Message>) {
        while let Ok(first) = incoming.recv() {
            for message in std::iter::once(first).chain(incoming.try_iter()) {
                self.handle(message);
            }
            self.tell();
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Search(asked, messages) => self.start(asked, messages),
            // Word of a query that a newer one replaced, or that has ended, changes nothing.
            Message::Word(id, word) => {
                if (self.current.as_ref()).is_some_and(|current| current.id == id) {
                    self.heard(id, word);
                }
            }
        }
    }

    /// Cancels the query running, and runs this one on a thread of its own, which sends word through
    /// `messages`.
    fn start(&mut self, asked: Asked, messages: Sender<Message>) {
        if let Some(current) = self.current.take() {
            current.cancelled.store(true, Ordering::Relaxed);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.current = Some(Current {
            id: asked.id,
            cancelled: Arc::clone(&cancelled),
        });
        self.state = SearchState::Results(Results::nothing_yet(asked.id));
        let requests = self.requests.clone();
        let _ = thread::spawn(move || run(&asked, &requests, &cancelled, &messages));
    }

    /// Takes `word` of query `id`, the one running.
    fn heard(&mut self, id: SearchId, word: Word) {
        match word {
            Word::Waiting => self.state = SearchState::Waiting { id },
            Word::Started => self.state = SearchState::Results(Results::nothing_yet(id)),
            Word::Found(batch) => {
                // Word of the files comes only once the query has started.
                if let SearchState::Results(results) = &mut self.state {
                    results.add(batch);
                }
            }
            Word::Ended(ending) => {
                self.current = None;
                match ending {
                    Ending::Finished(outcome) => {
                        if let SearchState::Results(results) = &mut self.state {
                            results.end = End::from(outcome);
                        }
                    }
                    Ending::NoCheckout => self.state = SearchState::NoCheckout { id },
                    Ending::Failed => self.state = SearchState::Failed { id },
                }
            }
        }
    }

    /// Hands the window the state, and says it may have changed.
    fn tell(&mut self) {
        self.writer.set(&self.state);
        (self.announce)();
    }
}

/// Searches `checkout` for `query` at once, on the threads a query runs on, and gives the files that hold a
/// match by path, and how the search ended. The window's queries go through a [`TextSearch`]; this serves
/// a benchmark.
pub fn search_checkout(
    checkout: &LookupCheckout,
    query: &Query,
) -> Result<(Vec<FileHits>, End), Error> {
    let matcher = engine::Matcher::new(query);
    let matcher = matcher.map_err(|bad| Error::new(ErrorKind::Internal, &bad.message))?;
    let mut files = Vec::new();
    let found = tree::search(checkout, &matcher, &AtomicBool::new(false), &mut |batch| {
        files.extend(batch);
    });
    let outcome = found.map_err(|cause| {
        log::warn!("Tsugiki's text search can't read the files of a Lookup checkout: {cause}");
        Error::new(ErrorKind::Internal, "Tsugiki couldn't read the PR's files.")
    })?;
    files.sort_by(|one, other| one.path.cmp(&other.path));
    Ok((files, End::from(outcome)))
}

/// Runs query `asked` in its Repository's Lookup checkout, once the checkout has stopped moving, and sends
/// the owner word of it. A query cancelled while it waited sends nothing more.
fn run(asked: &Asked, requests: &MirrorRequests, cancelled: &AtomicBool, owner: &Sender<Message>) {
    let Asked {
        id,
        repository,
        matcher,
    } = asked;
    // The owner has stopped only when the app quits, and then nobody reads what a search says.
    let tell = |word| {
        let _unheard = owner.send(Message::Word(*id, word));
    };
    let answer = requests.ask_lookup_checkout(repository.clone());
    let checkout = match answer.recv_timeout(WAIT_UNSAID) {
        Err(RecvTimeoutError::Timeout) => {
            tell(Word::Waiting);
            answer.recv().ok().flatten()
        }
        // A worker that stopped without an answer leaves no checkout either.
        answered => answered.ok().flatten(),
    };
    if cancelled.load(Ordering::Relaxed) {
        return;
    }
    let Some(checkout) = checkout else {
        return tell(Word::Ended(Ending::NoCheckout));
    };
    tell(Word::Started);
    let found = tree::search(&checkout, matcher, cancelled, &mut |batch| {
        tell(Word::Found(batch));
    });
    let ending = match found {
        Ok(outcome) => Ending::Finished(outcome),
        Err(cause) => {
            log::warn!("Tsugiki's text search can't read the files of {repository}'s PR: {cause}");
            Ending::Failed
        }
    };
    tell(Word::Ended(ending));
}

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
