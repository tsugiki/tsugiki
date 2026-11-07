//! Each Repository's **Mirror**: a bare clone in its folder (ADR 0002, ADR 0010). One worker thread writes
//! to the mirrors, beside the session's sync queue, and clones them one at a time.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader, Read};
use std::mem;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

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
type AnnounceCloning = Box<dyn FnMut(Option<&Cloning>) + Send>;

/// Where clones come from: GitHub, or in tests a folder of bare repositories laid out as GitHub's are.
pub struct Remotes {
    base: String,
}

impl Remotes {
    pub fn github() -> Self {
        Self::under("https://github.com/".to_owned())
    }

    /// Remotes at `base` followed by `owner/name.git`.
    pub fn under(base: String) -> Self {
        Self { base }
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

impl Mirrors {
    /// Starts the worker. Without `folders` it has nowhere to put a mirror, and logs each request instead.
    pub fn start(
        folders: Result<Folders, Error>,
        remotes: Remotes,
        announce: AnnounceCloning,
    ) -> Self {
        Self::with_git(Box::new(LiveGit), folders, remotes, announce)
    }

    fn with_git(
        git: Box<dyn Git>,
        folders: Result<Folders, Error>,
        remotes: Remotes,
        announce: AnnounceCloning,
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
    batch: BTreeMap<u32, CommitSha>,
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

/// What the worker knows of one Repository's PR heads.
struct Heads {
    repository: RepositoryName,
    /// Heads handed over and not fetched yet, which go in the next fetch.
    waiting: BTreeMap<u32, CommitSha>,
    /// The heads fetched since Tsugiki started. A head that moves on is fetched again.
    fetched: BTreeMap<u32, CommitSha>,
}

struct Worker {
    git: Box<dyn Git>,
    folders: Result<Folders, Error>,
    remotes: Remotes,
    announce: AnnounceCloning,
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
            match incoming.recv() {
                Ok(Message::Clone(repository, turn)) => self.wait(repository, turn),
                Ok(Message::Heads(repository, heads)) => {
                    self.heard_news();
                    if self.pace == Pace::Stalled {
                        self.pace = Pace::Going;
                    }
                    self.hand_over(&repository, heads);
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
                fetched: BTreeMap::new(),
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
        for PrHead { number, commit } in heads {
            if known.fetched.get(&number) == Some(&commit) {
                drop(known.waiting.remove(&number));
            } else {
                drop(known.waiting.insert(number, commit));
            }
        }
        self.heads.push_front(known);
    }

    /// Starts the next git, unless one is under way or GitHub can't be reached. A fetch goes ahead of a
    /// clone, as it takes a moment and a clone can take minutes.
    fn start_next(&mut self) {
        while self.job.is_none() && self.pace != Pace::Held {
            if let Some((repository, batch, mirror)) = self.next_batch() {
                self.start_fetch(repository, batch, &mirror);
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
                    (self.announce)(Some(&clone.cloning));
                    self.job = Some(Job::Clone(clone));
                }
                Err((repository, cause)) => {
                    log::warn!("Tsugiki can't start cloning {repository}'s mirror: {cause}");
                }
            }
        }
    }

    /// The waiting heads of the first Repository whose mirror is there, with where it is. The heads of one
    /// whose mirror isn't there wait for its clone.
    fn next_batch(&mut self) -> Option<(RepositoryName, BTreeMap<u32, CommitSha>, PathBuf)> {
        let going = self.pace == Pace::Going;
        let folders = self.folders.as_ref().ok().filter(|_| going)?;
        let heads = self.heads.iter_mut().find(|heads| {
            !heads.waiting.is_empty() && folders.mirror(&heads.repository).exists()
        })?;
        let mirror = folders.mirror(&heads.repository);
        Some((
            heads.repository.clone(),
            mem::take(&mut heads.waiting),
            mirror,
        ))
    }

    /// Fetches each head of `batch` in one go. A glob in each refspec lets a head GitHub no longer has match
    /// nothing, where a plain one would fail the whole fetch (#157). A connection that stalls for a minute
    /// fails the fetch, which would otherwise hold up every git after it.
    fn start_fetch(
        &mut self,
        repository: RepositoryName,
        batch: BTreeMap<u32, CommitSha>,
        mirror: &Path,
    ) {
        let refspecs: String = batch
            .keys()
            .map(|number| format!("+refs/pull/{number}/head*:refs/tsugiki/pull/{number}/head*\n"))
            .collect();
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
            OsStr::new("origin"),
        ];
        match self.git.start(&args, refspecs.as_bytes()) {
            Ok(started) => {
                self.listen(started.output);
                self.job = Some(Job::Fetch(Fetch {
                    repository,
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

    /// Puts back the heads of a fetch that failed, behind any newer ones handed over meanwhile.
    fn try_again_later(&mut self, repository: &RepositoryName, batch: BTreeMap<u32, CommitSha>) {
        let mut known = self.take_heads(repository);
        for (number, commit) in batch {
            let _ = known.waiting.entry(number).or_insert(commit);
        }
        self.heads.push_back(known);
    }

    /// Notes the heads a fetch brought, which no longer wait. A failed fetch leaves the refs it had, and its
    /// heads wait for the next fetch.
    fn finish_fetch(&mut self, fetch: Fetch, last_words: &str) {
        let Fetch {
            repository,
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
        let mut known = self.take_heads(&repository);
        known
            .waiting
            .retain(|number, commit| batch.get(number) != Some(commit));
        known.fetched.extend(batch);
        self.heads.push_back(known);
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
            (self.announce)(Some(&clone.cloning));
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
        (self.announce)(None);
    }
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
