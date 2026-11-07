//! Each Repository's **Mirror**: a bare clone in its folder (ADR 0002, ADR 0010). One worker thread writes
//! to the mirrors, beside the session's sync queue, and clones them one at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader, Read};
use std::mem;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;

use crate::error::Error;
use crate::folders::Folders;
use crate::github::pull_requests::PrHead;
use crate::github::{CommitSha, live};
use crate::repository::RepositoryName;
use crate::status::Status;

/// How far a clone has got, from what `git clone --progress` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum CloneProgress {
    /// GitHub is still counting and packing what it sends.
    Starting,
    /// The objects coming down.
    Receiving { percent: u8 },
    /// Indexing what came down, which takes about as long again.
    Resolving { percent: u8 },
}

/// The clone under way, for the status bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Cloning {
    pub repository: RepositoryName,
    pub progress: CloneProgress,
}

/// Where git clones to, in the Repository's folder, so a clone cut off never looks like a mirror.
const PARTIAL: &str = "mirror.git.partial";

/// Says the clone under way each time it moves on, and `None` when it ends.
pub type AnnounceCloning = Box<dyn FnMut(Option<&Cloning>) + Send>;

/// Says which PRs' heads a fetch brought into a Repository's mirror.
pub type AnnounceFetched = Box<dyn FnMut(&RepositoryName, &[u32]) + Send>;

/// What the worker tells the app as it goes.
pub struct Announce {
    pub cloning: AnnounceCloning,
    pub fetched: AnnounceFetched,
}

/// Where clones come from: GitHub, or in tests a folder of bare repositories laid out as GitHub's are.
pub struct Remotes {
    base: String,
    /// How long a head whose fetch brought another commit waits before it is fetched once more.
    patience: Duration,
}

impl Remotes {
    pub fn github() -> Self {
        Self::under("https://github.com/".to_owned())
    }

    /// Remotes at `base` followed by `owner/name.git`.
    pub fn under(base: String) -> Self {
        Self {
            base,
            patience: BEHIND_PATIENCE,
        }
    }

    /// The same remotes, with a head that is behind waiting `patience`.
    #[cfg(test)]
    fn patient(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    fn url(&self, repository: &RepositoryName) -> String {
        format!(
            "{}{}/{}.git",
            self.base,
            repository.owner(),
            repository.name()
        )
    }
}

enum Message {
    Clone(RepositoryName, Turn),
    /// The heads of the PRs the user's work reaches in a Repository, as the store has them now.
    Heads(RepositoryName, Vec<PrHead>),
    /// The head of a PR the user opened, which the mirror lacks.
    First(RepositoryName, PrHead),
    /// Whether to hold back the next git, while GitHub can't be reached.
    Hold(bool),
    Progress(CloneProgress),
    /// git closed its error output, so it has ended or is about to. Its last line that wasn't progress says
    /// why it failed.
    Ended {
        last_words: String,
    },
    Stop,
}

/// Where a Repository waits for its clone.
#[derive(Debug, Clone, Copy)]
enum Turn {
    Next,
    Last,
}

/// The mirrors' one writer.
pub struct Mirrors {
    messages: Sender<Message>,
    finished: Receiver<()>,
}

/// A way to ask for mirrors. It returns at once.
#[derive(Clone)]
pub struct MirrorRequests {
    messages: Sender<Message>,
}

impl MirrorRequests {
    /// Clones `repository`'s mirror after the ones already waiting, unless it has one.
    pub fn clone_mirror(&self, repository: RepositoryName) {
        self.send(Message::Clone(repository, Turn::Last));
    }

    /// Clones `repository`'s mirror before the ones waiting, as the open Repository's goes first. A clone
    /// under way finishes first.
    pub fn clone_mirror_next(&self, repository: RepositoryName) {
        self.send(Message::Clone(repository, Turn::Next));
    }

    /// Fetches the PR heads in `heads` into `repository`'s mirror, as `refs/tsugiki/pull/N/head`, once
    /// its mirror is there. Heads handed over while a fetch is under way go in the next one, all in one.
    pub fn fetch_heads(&self, repository: RepositoryName, heads: Vec<PrHead>) {
        self.send(Message::Heads(repository, heads));
    }

    /// Fetches the head of a PR the user opened, which `repository`'s mirror lacks, ahead of the heads
    /// waiting. The worker fetches it whatever it noted of it before, so a ref GitHub made late arrives.
    pub fn fetch_first(&self, repository: RepositoryName, head: PrHead) {
        self.send(Message::First(repository, head));
    }

    /// Holds back the next git while `status` says GitHub can't be reached. A rate limit is the API's, and
    /// git doesn't use the API, so it holds back nothing.
    pub fn follow(&self, status: &Status) {
        let held = match status {
            Status::Offline | Status::OfflineAs { .. } => true,
            Status::Connecting
            | Status::ConnectingAs { .. }
            | Status::Online { .. }
            | Status::Problem { .. } => false,
        };
        self.send(Message::Hold(held));
    }

    fn send(&self, message: Message) {
        if self.messages.send(message).is_err() {
            log::warn!("Tsugiki has stopped working on mirrors, so it can't take another request");
        }
    }
}

/// Requests that reach a test rather than a worker, which hears each head asked for first.
#[cfg(test)]
pub(crate) struct HeardRequests(Receiver<Message>);

#[cfg(test)]
impl HeardRequests {
    pub(crate) fn new() -> (MirrorRequests, Self) {
        let (messages, heard) = mpsc::channel();
        (MirrorRequests { messages }, Self(heard))
    }

    /// The heads asked for first since the last call.
    pub(crate) fn firsts(&self) -> Vec<(RepositoryName, PrHead)> {
        let heard = self.0.try_iter().filter_map(|message| match message {
            Message::First(repository, head) => Some((repository, head)),
            Message::Clone(..)
            | Message::Heads(..)
            | Message::Hold(_)
            | Message::Progress(_)
            | Message::Ended { .. }
            | Message::Stop => None,
        });
        heard.collect()
    }
}

impl Mirrors {
    /// Starts the worker. Without `folders` it has nowhere to put a mirror, and logs each request instead.
    pub fn start(folders: Result<Folders, Error>, remotes: Remotes, announce: Announce) -> Self {
        Self::with_git(Box::new(LiveGit), folders, remotes, announce)
    }

    fn with_git(
        git: Box<dyn Git>,
        folders: Result<Folders, Error>,
        remotes: Remotes,
        announce: Announce,
    ) -> Self {
        let (messages, incoming) = mpsc::channel();
        let (finishing, finished) = mpsc::channel::<()>();
        let mut worker = Worker {
            git,
            folders,
            remotes,
            announce,
            messages: messages.clone(),
            waiting: VecDeque::new(),
            heads: VecDeque::new(),
            pace: Pace::Going,
            job: None,
        };
        let _ = thread::spawn(move || {
            // `stop` learns the worker is over when this goes, however the thread ends. Locals go in reverse
            // order, so `incoming` goes first, and a request after `stop` finds nobody to take it.
            let _finishing = finishing;
            let incoming = incoming;
            worker.run(&incoming);
        });
        Self { messages, finished }
    }

    pub fn requests(&self) -> MirrorRequests {
        MirrorRequests {
            messages: self.messages.clone(),
        }
    }

    /// Stops a clone under way, with the helpers git started, and gives the worker up to `patience` to end.
    /// The next launch starts that clone again from an empty folder.
    pub fn stop(&self, patience: Duration) {
        self.begin_stopping();
        self.wait_for_end(patience);
    }

    /// Tells the worker to stop, and returns at once, so the caller can stop other threads meanwhile.
    pub fn begin_stopping(&self) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the worker has already finished, which is what this asks for"
        )]
        let _ = self.messages.send(Message::Stop);
    }

    /// Waits up to `patience` for the worker to end, after `begin_stopping`.
    pub fn wait_for_end(&self, patience: Duration) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "ending git takes a moment, so only a stuck disk outlasts `patience`, and quitting goes on"
        )]
        let _ = self.finished.recv_timeout(patience);
    }
}

/// The worker holds a sender for git's progress, so its messages never run out: without this, dropping
/// `Mirrors` would leave it, and a clone, running.
impl Drop for Mirrors {
    fn drop(&mut self) {
        self.begin_stopping();
    }
}

/// git as the worker runs it: the real one, or a fake in tests.
trait Git: Send {
    /// Starts git with `args`, and gives it `input` on its standard input.
    fn start(&self, args: &[&OsStr], input: &[u8]) -> io::Result<Started>;
    /// Runs a git that only reads a mirror, so it ends at once, and gives its output.
    fn read(&self, args: &[&OsStr]) -> io::Result<String>;
}

/// A git the worker started.
struct Started {
    run: Box<dyn Run>,
    /// git's error output, which it closes as it ends.
    output: Box<dyn Read + Send>,
}

/// A git under way.
trait Run: Send {
    /// Waits for git to end, and says if it did what it was asked.
    fn succeeded(&mut self) -> bool;
    /// Ends git and the helpers it started.
    fn stop(&mut self);
}

struct LiveGit;

impl Git for LiveGit {
    fn start(&self, args: &[&OsStr], input: &[u8]) -> io::Result<Started> {
        let (git, output) = live::start_git(args, input)?;
        Ok(Started {
            run: Box::new(git),
            output: Box::new(output),
        })
    }

    fn read(&self, args: &[&OsStr]) -> io::Result<String> {
        live::read_git(args)
    }
}

impl Run for Child {
    fn succeeded(&mut self) -> bool {
        self.wait().is_ok_and(|status| status.success())
    }

    fn stop(&mut self) {
        live::stop_git(self);
    }
}

/// The one git under way.
enum Job {
    Clone(Clone),
    Fetch(Fetch),
}

/// A clone under way.
struct Clone {
    cloning: Cloning,
    git: Box<dyn Run>,
    partial: PathBuf,
    mirror: PathBuf,
}

/// A fetch of PR heads under way.
struct Fetch {
    repository: RepositoryName,
    mirror: PathBuf,
    batch: Batch,
    git: Box<dyn Run>,
    /// Heads or a status came while it ran, so if it fails, the next fetch has a reason to try again.
    news: bool,
}

/// Which git may start next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pace {
    Going,
    /// The last fetch failed, so the next waits for news: heads handed over, or a new status. Clones go on.
    Stalled,
    /// GitHub can't be reached, so no git starts.
    Held,
}

/// How long a head whose fetch brought another commit waits before it is fetched once more. GitHub moves
/// `refs/pull/N/head` a few seconds after a push.
const BEHIND_PATIENCE: Duration = Duration::from_secs(10);

/// What one fetch brings into a mirror: PR heads, and the Target branches their diffs start from.
#[derive(Debug, Default)]
struct Batch {
    heads: BTreeMap<u32, CommitSha>,
    branches: BTreeSet<String>,
}

/// What the worker knows of one Repository's PR heads.
struct Heads {
    repository: RepositoryName,
    /// Heads handed over and not fetched yet, which go in the next fetch.
    waiting: BTreeMap<u32, CommitSha>,
    /// The Target branches of the heads waiting, which the next fetch brings up to date with them.
    branches: BTreeSet<String>,
    /// Heads whose fetch left their ref somewhere else, as GitHub hadn't moved it yet (#201), or whose ref
    /// couldn't be read back.
    behind: BTreeMap<u32, Behind>,
    /// The heads fetched since Tsugiki started, and those GitHub didn't have. A head that moves on is
    /// fetched again.
    settled: BTreeMap<u32, CommitSha>,
}

impl Heads {
    /// Notes where the heads of a fetch that succeeded `landed`, or nothing if that couldn't be read, and
    /// gives the PRs whose heads landed. One that is behind for the first time since it was handed over is
    /// fetched once more at `retry`.
    fn note_fetched(
        &mut self,
        batch: BTreeMap<u32, CommitSha>,
        landed: Option<&BTreeMap<u32, CommitSha>>,
        retry: Instant,
    ) -> Vec<u32> {
        let mut arrived = Vec::new();
        for (number, commit) in batch {
            // The same head handed over during the fetch asks for nothing more.
            if self.waiting.get(&number) == Some(&commit) {
                drop(self.waiting.remove(&number));
            }
            let elsewhere = match landed {
                Some(landed) => landed.get(&number).filter(|&at| *at != commit),
                None => Some(&commit),
            };
            let Some(elsewhere) = elsewhere else {
                if landed.is_some_and(|landed| landed.contains_key(&number)) {
                    arrived.push(number);
                }
                drop(self.behind.remove(&number));
                drop(self.settled.insert(number, commit));
                continue;
            };
            if landed.is_some() {
                let (repository, had, handed) =
                    (&self.repository, elsewhere.as_str(), commit.as_str());
                log::info!("GitHub had PR #{number} of {repository} at {had} rather than {handed}");
            }
            let retried = Behind::Retried {
                commit: commit.clone(),
            };
            if self.behind.get(&number) != Some(&retried) {
                let due = Behind::Due { commit, at: retry };
                drop(self.behind.insert(number, due));
            }
        }
        arrived
    }
}

/// A head whose fetch didn't bring it. It is fetched once more after a moment, and after that only when
/// it is handed over again, so a ref GitHub never moves doesn't keep a fetch going.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Behind {
    Due { commit: CommitSha, at: Instant },
    Retried { commit: CommitSha },
}

struct Worker {
    git: Box<dyn Git>,
    folders: Result<Folders, Error>,
    remotes: Remotes,
    announce: Announce,
    /// For the thread that reads each git's progress.
    messages: Sender<Message>,
    waiting: VecDeque<RepositoryName>,
    /// The Repository handed heads last comes first.
    heads: VecDeque<Heads>,
    pace: Pace,
    job: Option<Job>,
}

impl Worker {
    fn run(&mut self, incoming: &Receiver<Message>) {
        loop {
            self.start_next();
            let message = match self.next_due() {
                Some(at) => {
                    match incoming.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Err(RecvTimeoutError::Timeout) => continue,
                        other => other.map_err(drop),
                    }
                }
                None => incoming.recv().map_err(drop),
            };
            match message {
                Ok(Message::Clone(repository, turn)) => self.wait(repository, turn),
                Ok(Message::Heads(repository, heads)) => {
                    self.heard_news();
                    if self.pace == Pace::Stalled {
                        self.pace = Pace::Going;
                    }
                    self.hand_over(&repository, heads);
                }
                Ok(Message::First(repository, head)) => {
                    self.heard_news();
                    if self.pace == Pace::Stalled {
                        self.pace = Pace::Going;
                    }
                    self.put_first(&repository, head);
                }
                Ok(Message::Hold(held)) => {
                    self.heard_news();
                    self.pace = if held { Pace::Held } else { Pace::Going };
                }
                Ok(Message::Progress(progress)) => self.moved_on(progress),
                // Each git's end comes while its job is the one under way.
                Ok(Message::Ended { last_words }) => {
                    if let Some(job) = self.job.take() {
                        self.finish_job(job, &last_words);
                    }
                }
                Ok(Message::Stop) | Err(_) => break,
            }
        }
        match self.job.take() {
            Some(Job::Clone(Clone { mut git, .. }) | Job::Fetch(Fetch { mut git, .. })) => {
                git.stop()
            }
            None => {}
        }
    }

    fn wait(&mut self, repository: RepositoryName, turn: Turn) {
        let cloning = match &self.job {
            Some(Job::Clone(clone)) => Some(&clone.cloning.repository),
            Some(Job::Fetch(_)) | None => None,
        };
        if cloning.is_some_and(|cloning| cloning.is_same(&repository)) {
            return;
        }
        let waiting = self.waiting.iter().position(|one| one.is_same(&repository));
        match (turn, waiting) {
            (Turn::Last, Some(_)) => {}
            (Turn::Last, None) => self.waiting.push_back(repository),
            (Turn::Next, waiting) => {
                if let Some(index) = waiting {
                    drop(self.waiting.remove(index));
                }
                self.waiting.push_front(repository);
            }
        }
    }

    fn finish_job(&mut self, job: Job, last_words: &str) {
        match job {
            Job::Clone(clone) => self.finish(clone, last_words),
            Job::Fetch(fetch) => self.finish_fetch(fetch, last_words),
        }
    }

    /// Takes out what the worker knows of `repository`'s heads, which the caller puts back.
    fn take_heads(&mut self, repository: &RepositoryName) -> Heads {
        self.heads
            .iter()
            .position(|known| known.repository.is_same(repository))
            .and_then(|at| self.heads.remove(at))
            .unwrap_or_else(|| Heads {
                repository: repository.clone(),
                waiting: BTreeMap::new(),
                branches: BTreeSet::new(),
                behind: BTreeMap::new(),
                settled: BTreeMap::new(),
            })
    }

    fn heard_news(&mut self) {
        if let Some(Job::Fetch(fetch)) = &mut self.job {
            fetch.news = true;
        }
    }

    /// Keeps each head of `heads` that the mirror may not have yet, for the next fetch. One that the fetch
    /// under way asks for too waits, in case that fetch fails.
    fn hand_over(&mut self, repository: &RepositoryName, heads: Vec<PrHead>) {
        let mut known = self.take_heads(repository);
        for PrHead {
            number,
            commit,
            target_branch,
        } in heads
        {
            // A hand-over is a reason to try a head again, so one that was behind starts afresh.
            drop(known.behind.remove(&number));
            if known.settled.get(&number) == Some(&commit) {
                drop(known.waiting.remove(&number));
            } else {
                drop(known.waiting.insert(number, commit));
                let _ = known.branches.insert(target_branch);
            }
        }
        self.heads.push_front(known);
    }

    /// Puts the head of a PR the user opened in the next fetch, whatever the worker noted of it, and its
    /// Repository ahead of the others.
    fn put_first(&mut self, repository: &RepositoryName, head: PrHead) {
        let mut known = self.take_heads(repository);
        drop(known.behind.remove(&head.number));
        drop(known.settled.remove(&head.number));
        drop(known.waiting.insert(head.number, head.commit));
        let _ = known.branches.insert(head.target_branch);
        self.heads.push_front(known);
    }

    /// Starts the next git, unless one is under way or GitHub can't be reached. A fetch goes ahead of a
    /// clone, as it takes a moment and a clone can take minutes.
    fn start_next(&mut self) {
        self.retry_due();
        while self.job.is_none() && self.pace != Pace::Held {
            if let Some((repository, batch, mirror)) = self.next_batch() {
                self.start_fetch(repository, batch, mirror);
                continue;
            }
            let Some(repository) = self.waiting.pop_front() else {
                return;
            };
            let folders = match &self.folders {
                Ok(folders) => folders,
                Err(problem) => {
                    log::warn!(
                        "Tsugiki can't clone {repository}'s mirror: {}",
                        problem.message
                    );
                    continue;
                }
            };
            let folder = folders.repository(&repository);
            if folders.mirror(&repository).exists() {
                continue;
            }
            match self.start_clone(repository, folder) {
                Ok(clone) => {
                    (self.announce.cloning)(Some(&clone.cloning));
                    self.job = Some(Job::Clone(clone));
                }
                Err((repository, cause)) => {
                    log::warn!("Tsugiki can't start cloning {repository}'s mirror: {cause}");
                }
            }
        }
    }

    /// When the first head that is behind is due to be fetched once more.
    fn next_due(&self) -> Option<Instant> {
        self.heads
            .iter()
            .flat_map(|heads| heads.behind.values())
            .filter_map(|behind| match behind {
                Behind::Due { at, .. } => Some(*at),
                Behind::Retried { .. } => None,
            })
            .min()
    }

    /// Puts each head that is due to be fetched once more with the waiting ones. A newer one handed over
    /// meanwhile went there itself.
    fn retry_due(&mut self) {
        let now = Instant::now();
        for heads in &mut self.heads {
            for (number, behind) in &mut heads.behind {
                if let Behind::Due { commit, at } = behind
                    && *at <= now
                {
                    let _ = heads
                        .waiting
                        .entry(*number)
                        .or_insert_with(|| commit.clone());
                    *behind = Behind::Retried {
                        commit: commit.clone(),
                    };
                }
            }
        }
    }

    /// The waiting heads of the first Repository whose mirror is there, with where it is. The heads of one
    /// whose mirror isn't there wait for its clone.
    fn next_batch(&mut self) -> Option<(RepositoryName, Batch, PathBuf)> {
        let going = self.pace == Pace::Going;
        let folders = self.folders.as_ref().ok().filter(|_| going)?;
        let heads = self.heads.iter_mut().find(|heads| {
            !heads.waiting.is_empty() && folders.mirror(&heads.repository).exists()
        })?;
        let mirror = folders.mirror(&heads.repository);
        let batch = Batch {
            heads: mem::take(&mut heads.waiting),
            branches: mem::take(&mut heads.branches),
        };
        Some((heads.repository.clone(), batch, mirror))
    }

    /// Fetches each head and branch of `batch` in one go. A glob in each refspec lets a head or a branch
    /// GitHub no longer has match nothing, where a plain one would fail the whole fetch (#157), and
    /// `--prune` deletes its old ref, so the refs read back say which heads GitHub has. A branch's glob
    /// also brings the branches whose names start with its name, which costs little. A connection that
    /// stalls for a minute fails the fetch, which would otherwise hold up every git after it.
    fn start_fetch(&mut self, repository: RepositoryName, batch: Batch, mirror: PathBuf) {
        let heads = batch
            .heads
            .keys()
            .map(|number| format!("+refs/pull/{number}/head*:refs/tsugiki/pull/{number}/head*\n"));
        let branches = (batch.branches.iter())
            .map(|branch| format!("+refs/heads/{branch}*:refs/heads/{branch}*\n"));
        let refspecs: String = heads.chain(branches).collect();
        let args = [
            OsStr::new("-c"),
            OsStr::new("http.lowSpeedLimit=1"),
            OsStr::new("-c"),
            OsStr::new("http.lowSpeedTime=60"),
            OsStr::new("-C"),
            mirror.as_os_str(),
            OsStr::new("fetch"),
            OsStr::new("--stdin"),
            OsStr::new("--no-tags"),
            OsStr::new("--no-write-fetch-head"),
            OsStr::new("--prune"),
            OsStr::new("origin"),
        ];
        match self.git.start(&args, refspecs.as_bytes()) {
            Ok(started) => {
                self.listen(started.output);
                self.job = Some(Job::Fetch(Fetch {
                    repository,
                    mirror,
                    batch,
                    git: started.run,
                    news: false,
                }));
            }
            Err(cause) => {
                log::warn!("Tsugiki can't start fetching PR heads of {repository}: {cause}");
                self.try_again_later(&repository, batch);
                self.pace = Pace::Stalled;
            }
        }
    }

    /// Puts back the heads and branches of a fetch that failed, behind any newer heads handed over
    /// meanwhile.
    fn try_again_later(&mut self, repository: &RepositoryName, batch: Batch) {
        let mut known = self.take_heads(repository);
        for (number, commit) in batch.heads {
            let _ = known.waiting.entry(number).or_insert(commit);
        }
        known.branches.extend(batch.branches);
        self.heads.push_back(known);
    }

    /// Notes the heads a fetch brought, which no longer wait. A failed fetch leaves the refs it had, and its
    /// heads wait for the next fetch.
    ///
    /// GitHub moves `refs/pull/N/head` a moment after a push, so a fetch can bring the commit before the
    /// one handed over (#201). A head whose ref doesn't point at its commit is behind, and so is each head
    /// of a fetch whose refs can't be read back. A head whose glob matched nothing has no ref, as it isn't
    /// on GitHub, and is settled until it moves on.
    fn finish_fetch(&mut self, fetch: Fetch, last_words: &str) {
        let Fetch {
            repository,
            mirror,
            batch,
            mut git,
            news,
        } = fetch;
        if !git.succeeded() {
            log::warn!("Tsugiki couldn't fetch PR heads of {repository}: {last_words}");
            self.try_again_later(&repository, batch);
            // A fetch starts only while the worker is going, and no news since means it still is. Fetches
            // wait for some, so one that can't work doesn't run again and again.
            if !news {
                self.pace = Pace::Stalled;
            }
            return;
        }
        let landed = match self.read_pull_refs(&mirror) {
            Ok(landed) => Some(landed),
            Err(cause) => {
                log::warn!("Tsugiki can't read which PR heads of {repository} it fetched: {cause}");
                None
            }
        };
        let retry = Instant::now() + self.remotes.patience;
        // Back where it was, so an opened PR's head handed over meanwhile still goes first.
        let at = self
            .heads
            .iter()
            .position(|known| known.repository.is_same(&repository));
        let mut known = self.take_heads(&repository);
        let arrived = known.note_fetched(batch.heads, landed.as_ref(), retry);
        self.heads.insert(at.unwrap_or(self.heads.len()), known);
        if !arrived.is_empty() {
            (self.announce.fetched)(&repository, &arrived);
        }
    }

    /// Where each PR head ref in `mirror` points.
    fn read_pull_refs(&self, mirror: &Path) -> io::Result<BTreeMap<u32, CommitSha>> {
        let args = [
            OsStr::new("-C"),
            mirror.as_os_str(),
            OsStr::new("for-each-ref"),
            OsStr::new("--format=%(objectname) %(refname)"),
            OsStr::new("refs/tsugiki/pull/"),
        ];
        Ok(pull_refs(&self.git.read(&args)?))
    }

    fn start_clone(
        &self,
        repository: RepositoryName,
        folder: PathBuf,
    ) -> Result<Clone, (RepositoryName, io::Error)> {
        let partial = folder.join(PARTIAL);
        let mirror = folder.join("mirror.git");
        match self.start_clone_git(&self.remotes.url(&repository), &folder, &partial) {
            Ok(started) => {
                self.listen(started.output);
                Ok(Clone {
                    cloning: Cloning {
                        repository,
                        progress: CloneProgress::Starting,
                    },
                    git: started.run,
                    partial,
                    mirror,
                })
            }
            Err(cause) => Err((repository, cause)),
        }
    }

    /// Starts `git clone` into `partial`, in the Repository's `folder`, deleting what a clone cut off by
    /// quitting left there.
    fn start_clone_git(&self, url: &str, folder: &Path, partial: &Path) -> io::Result<Started> {
        if partial.exists() {
            fs::remove_dir_all(partial)?;
        }
        fs::create_dir_all(folder)?;
        let args = ["clone", "--bare", "--no-tags", "--progress", url].map(OsStr::new);
        self.git
            .start(&[args.as_slice(), &[partial.as_os_str()]].concat(), &[])
    }

    /// Reads git's error output on a thread of its own, which tells the worker when git ends.
    fn listen(&self, output: Box<dyn Read + Send>) {
        let messages = self.messages.clone();
        let _ = thread::spawn(move || read_progress_until_done(output, &messages));
    }

    fn moved_on(&mut self, progress: CloneProgress) {
        if let Some(Job::Clone(clone)) = &mut self.job
            && clone.cloning.progress != progress
        {
            clone.cloning.progress = progress;
            (self.announce.cloning)(Some(&clone.cloning));
        }
    }

    /// Moves a finished clone to where a mirror goes, or deletes what a failed one left. git's last words
    /// say why it failed.
    fn finish(&mut self, mut clone: Clone, last_words: &str) {
        let repository = &clone.cloning.repository;
        let ended = if clone.git.succeeded() {
            fs::rename(&clone.partial, &clone.mirror)
        } else {
            Err(io::Error::other(last_words.to_owned()))
        };
        match ended {
            Ok(()) => log::info!("Tsugiki cloned {repository}'s mirror"),
            Err(why) => {
                log::warn!("Tsugiki couldn't clone {repository}'s mirror: {why}");
                // What is left goes before the next clone of this Repository, which says if it can't.
                drop(fs::remove_dir_all(&clone.partial));
            }
        }
        (self.announce.cloning)(None);
    }
}

/// Reads `git for-each-ref`'s lines of a commit and a ref. A ref the glob matched beside the head, such as
/// `refs/tsugiki/pull/1/head-x`, isn't one.
fn pull_refs(listing: &str) -> BTreeMap<u32, CommitSha> {
    listing
        .lines()
        .filter_map(|line| {
            let (commit, name) = line.split_once(' ')?;
            let number = name
                .strip_prefix("refs/tsugiki/pull/")?
                .strip_suffix("/head")?
                .parse()
                .ok()?;
            Some((number, CommitSha::parse(commit)?))
        })
        .collect()
}

/// Passes on each line of progress git writes, until git closes its error output. git ends a line of
/// progress with a carriage return, and other lines with a newline.
fn read_progress_until_done(progress: Box<dyn Read + Send>, messages: &Sender<Message>) {
    let mut last_words = String::new();
    let mut line = Vec::new();
    for byte in BufReader::new(progress).bytes().map_while(Result::ok) {
        if byte != b'\r' && byte != b'\n' {
            line.push(byte);
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        match read_progress(&text) {
            Some(progress) => drop(messages.send(Message::Progress(progress))),
            None if !text.trim().is_empty() => last_words = text.trim().to_owned(),
            None => {}
        }
        line.clear();
    }
    drop(messages.send(Message::Ended { last_words }));
}

/// Reads one line of `git clone --progress`. Lines from the server, and lines without a percentage, say
/// nothing about the clone here.
fn read_progress(line: &str) -> Option<CloneProgress> {
    let (stage, rest) = line.split_once(':')?;
    let (number, _) = rest.trim_start().split_once('%')?;
    let percent = number.parse().ok().filter(|percent| *percent <= 100)?;
    match stage {
        "Receiving objects" => Some(CloneProgress::Receiving { percent }),
        "Resolving deltas" => Some(CloneProgress::Resolving { percent }),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
