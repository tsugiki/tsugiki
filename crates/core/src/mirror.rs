//! Each Repository's **Mirror**: a bare clone in its folder (ADR 0002, ADR 0010). One worker thread writes
//! to the mirrors, beside the session's sync queue, and clones them one at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader, Read};
use std::mem;
use std::ops::ControlFlow;
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
use crate::github::{CommitSha, GitHub, Transport, live};
use crate::repository::RepositoryName;
use crate::status::Status;
use crate::token::{RunGh, Token};

mod source;
mod upkeep;
pub use source::{CloneRefusal, MirrorSource, MirrorSources};
use source::{GITHUB_PREFIXES, lender};
use upkeep::{Schedule, Upkeep, Upkept, delete_broken, says_broken};

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

/// The reason no mirror can be made, for the status bar (#171). The rest of Tsugiki works without one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MirrorProblem {
    /// git isn't on the `PATH`.
    NoGit,
    /// git is older than 2.30, as `git --version` says in `version`.
    OldGit { version: String },
    /// The disk that holds `folder` has `free_mb` megabytes free, short of the `needed_mb` a clone of
    /// `repository` takes.
    DiskShort {
        repository: RepositoryName,
        folder: String,
        needed_mb: u32,
        free_mb: u32,
    },
}

/// The oldest git a mirror works with (ADR 0003): `git check-attr --source` and the partial clone's
/// options came in 2.30 (#166).
const OLDEST_GIT: (u32, u32) = (2, 30);

/// The room a clone needs beyond the Repository's size on GitHub: git's index of the pack, and the temporary
/// files it writes as it goes.
const ROOM_TO_SPARE: u64 = 1 << 30;

/// The wait before a clone that failed goes back in line, about as long as the status takes to look at
/// GitHub again.
const CLONE_RETRY: Duration = Duration::from_secs(60);

/// How many times a clone's wait doubles as it keeps failing: 60 s grows to about an hour.
const MAX_DOUBLINGS: u32 = 6;

/// The size in bytes of a Repository on GitHub.
pub type SizeOf = Box<dyn FnMut(&RepositoryName) -> Result<u64, Error> + Send>;

/// The bytes free on the disk that holds a folder.
pub type FreeSpace = Box<dyn Fn(&Path) -> io::Result<u64> + Send>;

/// Asks GitHub for a Repository's size, through a new connection signed in with gh's token each time, as a
/// clone is rare and the session owns the other one.
pub fn sizes_on_github(new_transport: fn() -> Box<dyn Transport>, run_gh: RunGh) -> SizeOf {
    Box::new(move |repository| {
        let token = Token::from_gh(run_gh)?;
        GitHub::new(new_transport(), token).repository_size(repository)
    })
}

/// A connection to GitHub's API, for a Repository's size.
fn https() -> Box<dyn Transport> {
    Box::new(live::Https::new())
}

/// Where git clones to, in the Repository's folder, so a clone cut off never looks like a mirror.
const PARTIAL: &str = "mirror.git.partial";

/// Says the clone under way each time it moves on, and `None` when it ends.
pub type AnnounceCloning = Box<dyn FnMut(Option<&Cloning>) + Send>;

/// Says which PRs' heads a fetch brought into a Repository's mirror.
pub type AnnounceFetched = Box<dyn FnMut(&RepositoryName, &[u32]) + Send>;

/// Says which PRs' heads a fetch into a Repository's mirror failed to bring, and why, once they wait for
/// news or a hand-over before the next fetch.
pub type AnnounceFailed = Box<dyn FnMut(&RepositoryName, &[u32], &Unfetched) + Send>;

/// The reason PR heads aren't in a mirror once the worker stopped fetching them, for a Files tab waiting for
/// one (#233, #213).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Unfetched {
    /// git failed, with its last line in `words`: the fetch, or reading back where its refs landed.
    Git { words: String },
    /// GitHub's ref for the PR points at another commit. GitHub hadn't moved it yet (#201), or the head
    /// the last sync stored is no longer on the PR's branch.
    Behind,
    /// GitHub has no ref for the PR.
    #[serde(rename = "not_on_github")]
    NotOnGitHub,
}

/// Says why no mirror can be made, and `None` once one can.
pub type AnnounceProblem = Box<dyn FnMut(Option<&MirrorProblem>) + Send>;

/// Names the first Repository whose clone waits for the user to say where it gets its objects, and `None`
/// once none waits.
pub type AnnounceQuestion = Box<dyn FnMut(Option<&RepositoryName>) + Send>;

/// What the worker tells the app as it goes.
pub struct Announce {
    pub cloning: AnnounceCloning,
    pub fetched: AnnounceFetched,
    pub failed: AnnounceFailed,
    pub problem: AnnounceProblem,
    pub question: AnnounceQuestion,
}

impl Announce {
    /// Says only the clone under way, for an app or a test that shows nothing of the heads fetched.
    pub fn cloning_only(cloning: AnnounceCloning) -> Self {
        Self {
            cloning,
            fetched: Box::new(|_, _| {}),
            failed: Box::new(|_, _, _| {}),
            problem: Box::new(|_| {}),
            question: Box::new(|_| {}),
        }
    }
}

/// Where clones come from: GitHub, or in tests a folder of bare repositories laid out as GitHub's are. It
/// also says how big a clone is, and how much room its disk has.
pub struct Remotes {
    base: String,
    /// The start of a remote of a Repository at `base`, in a clone the user has.
    prefixes: Vec<String>,
    /// How long a head whose fetch brought another commit waits before it is fetched once more.
    patience: Duration,
    /// The wait before a clone that failed goes back in line.
    retry: Duration,
    size_of: SizeOf,
    free_space: FreeSpace,
    /// The schedule of each mirror's upkeep, or nothing for mirrors left as they are.
    upkeep: Option<Schedule>,
}

impl Remotes {
    /// GitHub's remotes, whose sizes its API gives, signed in with gh's token.
    pub fn github() -> Self {
        Self {
            size_of: sizes_on_github(https, live::run_gh),
            free_space: Box::new(|folder| fs4::available_space(folder)),
            prefixes: GITHUB_PREFIXES.map(str::to_owned).to_vec(),
            upkeep: Some(Schedule::DAILY_AND_WEEKLY),
            ..Self::under("https://github.com/".to_owned())
        }
    }

    /// Remotes at `base` followed by `owner/name.git`, which take no room, with mirrors left as they are.
    pub fn under(base: String) -> Self {
        Self {
            prefixes: vec![base.clone()],
            base,
            patience: BEHIND_PATIENCE,
            retry: CLONE_RETRY,
            size_of: Box::new(|_| Ok(0)),
            free_space: Box::new(|_| Ok(u64::MAX)),
            upkeep: None,
        }
    }

    /// The same remotes, with a head that is behind waiting `patience`.
    #[cfg(test)]
    fn patient(self, patience: Duration) -> Self {
        Self { patience, ..self }
    }

    /// The same remotes, with a clone that failed waiting `retry`.
    #[cfg(test)]
    fn retrying(self, retry: Duration) -> Self {
        Self { retry, ..self }
    }

    /// The same remotes, with each mirror's upkeep on `schedule`.
    #[cfg(test)]
    fn keeping(self, schedule: Schedule) -> Self {
        Self {
            upkeep: Some(schedule),
            ..self
        }
    }

    /// The same remotes, sized by `size_of` on a disk that `free_space` measures, for tests.
    pub fn on_disk(self, size_of: SizeOf, free_space: FreeSpace) -> Self {
        Self {
            size_of,
            free_space,
            ..self
        }
    }

    /// The URL a clone of `repository` comes from.
    pub fn url(&self, repository: &RepositoryName) -> String {
        format!(
            "{}{}/{}.git",
            self.base,
            repository.owner(),
            repository.name()
        )
    }
}

enum Message {
    /// A clone to make, with the user's answer of where it gets its objects, if they have given one.
    Clone(RepositoryName, Turn, Option<MirrorSource>),
    /// The heads of the PRs the user's work reaches in a Repository, as the store has them now.
    Heads(RepositoryName, Vec<PrHead>),
    /// The head of a PR the user opened, which the mirror lacks.
    First(RepositoryName, PrHead),
    /// A PR the user opened, whose ref stays until the next launch.
    Opened(RepositoryName, u32),
    /// Whether to hold back the next git, while GitHub can't be reached.
    Hold(bool),
    /// The user asks the worker to look at git and the disk again.
    CheckAgain,
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
    prefixes: Vec<String>,
}

/// A way to ask for mirrors. It returns at once, but for `check_clone`.
#[derive(Clone)]
pub struct MirrorRequests {
    messages: Sender<Message>,
    /// The remotes' prefixes, so a clone the user picks is checked as the worker will check it.
    prefixes: Vec<String>,
}

impl MirrorRequests {
    /// Clones `repository`'s mirror after the ones already waiting, unless it has one. With no `source`, the
    /// clone waits until the user says where it gets its objects, or a request with one comes.
    pub fn clone_mirror(&self, repository: RepositoryName, source: Option<MirrorSource>) {
        self.send(Message::Clone(repository, Turn::Last, source));
    }

    /// Clones `repository`'s mirror before the ones waiting, as the open Repository's goes first. A clone
    /// under way finishes first.
    pub fn clone_mirror_next(&self, repository: RepositoryName, source: Option<MirrorSource>) {
        self.send(Message::Clone(repository, Turn::Next, source));
    }

    /// Says whether the clone in `folder` can lend its objects to `repository`'s mirror. It runs git commands
    /// that only read the clone, and waits for them.
    pub fn check_clone(
        &self,
        folder: &Path,
        repository: &RepositoryName,
    ) -> Result<(), CloneRefusal> {
        lender(&LiveGit, &self.prefixes, folder, repository).map(drop)
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

    /// Keeps the ref of a PR the user opened until the next launch, though the user's work may not reach it.
    pub fn opened(&self, repository: RepositoryName, number: u32) {
        self.send(Message::Opened(repository, number));
    }

    /// Looks at git and the disk again, and tries each clone that waits for them or failed.
    pub fn check_again(&self) {
        self.send(Message::CheckAgain);
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
        let requests = MirrorRequests {
            messages,
            prefixes: Vec::new(),
        };
        (requests, Self(heard))
    }

    /// The heads asked for first since the last call.
    pub(crate) fn firsts(&self) -> Vec<(RepositoryName, PrHead)> {
        let heard = self.0.try_iter().filter_map(|message| match message {
            Message::First(repository, head) => Some((repository, head)),
            Message::Clone(..)
            | Message::Heads(..)
            | Message::Opened(..)
            | Message::Hold(_)
            | Message::CheckAgain
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
        let prefixes = remotes.prefixes.clone();
        let mut worker = Worker {
            git,
            folders,
            remotes,
            announce,
            messages: messages.clone(),
            waiting: VecDeque::new(),
            failed: Vec::new(),
            clone_tries: Vec::new(),
            heads: VecDeque::new(),
            pace: Pace::Going,
            job: None,
            git_checked: false,
            problem: None,
            told: None,
            sources: MirrorSources::default(),
            asking: Vec::new(),
            told_asking: None,
            upkept: Vec::new(),
            replaced: Vec::new(),
        };
        let _ = thread::spawn(move || {
            // `stop` learns the worker is over when this goes, however the thread ends. Locals go in reverse
            // order, so `incoming` goes first, and a request after `stop` finds nobody to take it.
            let _finishing = finishing;
            let incoming = incoming;
            worker.run(&incoming);
        });
        Self {
            messages,
            finished,
            prefixes,
        }
    }

    pub fn requests(&self) -> MirrorRequests {
        MirrorRequests {
            messages: self.messages.clone(),
            prefixes: self.prefixes.clone(),
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
    Upkeep(Upkeep),
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
    /// An opened PR's head waits, which goes ahead of the other Repositories' heads, even when this
    /// Repository's fetch is under way. The next fetch takes it.
    opened: bool,
    /// The PRs of the last hand-over: those the user's work reaches. Nothing before the first one.
    handed: Option<BTreeSet<u32>>,
    /// The PRs the user opened since launch, whose refs stay though the user's work doesn't reach them.
    viewed: BTreeSet<u32>,
}

/// The outcome of a fetch that succeeded, for the heads it asked for.
#[derive(Debug, Default)]
struct Noted {
    /// The PRs whose heads landed.
    arrived: Vec<u32>,
    /// The PRs whose heads didn't land once more, by why, which now wait for a hand-over.
    stuck: BTreeMap<Unfetched, Vec<u32>>,
}

impl Heads {
    /// Notes where the heads of a fetch that succeeded `landed`, or git's words if that couldn't be read.
    /// A head that didn't land for the first time since it was handed over is fetched once more at `retry`.
    fn note_fetched(
        &mut self,
        batch: BTreeMap<u32, CommitSha>,
        landed: Result<&BTreeMap<u32, CommitSha>, &str>,
        retry: Instant,
    ) -> Noted {
        let mut noted = Noted::default();
        for (number, commit) in batch {
            // The same head handed over during the fetch asks for nothing more.
            if self.waiting.get(&number) == Some(&commit) {
                drop(self.waiting.remove(&number));
            }
            let why = match landed {
                Ok(landed) => match landed.get(&number) {
                    Some(at) if *at == commit => {
                        noted.arrived.push(number);
                        drop(self.behind.remove(&number));
                        drop(self.settled.insert(number, commit));
                        continue;
                    }
                    Some(at) => {
                        let (repository, had, handed) =
                            (&self.repository, at.as_str(), commit.as_str());
                        log::info!(
                            "GitHub had PR #{number} of {repository} at {had} rather than {handed}"
                        );
                        Unfetched::Behind
                    }
                    None => Unfetched::NotOnGitHub,
                },
                Err(words) => Unfetched::Git {
                    words: words.to_owned(),
                },
            };
            let retried = Behind::Retried {
                commit: commit.clone(),
            };
            if self.behind.get(&number) != Some(&retried) {
                let due = Behind::Due { commit, at: retry };
                drop(self.behind.insert(number, due));
                continue;
            }
            // A head GitHub doesn't have is settled until it moves on, so a hand-over doesn't fetch it.
            if why == Unfetched::NotOnGitHub {
                drop(self.behind.remove(&number));
                drop(self.settled.insert(number, commit));
            }
            noted.stuck.entry(why).or_default().push(number);
        }
        noted
    }
}

/// A head whose fetch didn't bring it. It is fetched once more after a moment, and after that only when
/// it is handed over again, so a ref GitHub never moves doesn't keep a fetch going. The app hears of it
/// after that second fetch.
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
    /// The Repositories whose clone failed, each with the time it goes back in line. A Repository is in
    /// one of `waiting`, `failed` and the clone under way at most.
    failed: Vec<(RepositoryName, Instant)>,
    /// How many times in a row each Repository's clone has failed, which sets its next wait.
    clone_tries: Vec<(RepositoryName, u32)>,
    /// The Repository handed heads last comes first.
    heads: VecDeque<Heads>,
    pace: Pace,
    job: Option<Job>,
    /// Whether `git --version` has run since launch or the last Check again.
    git_checked: bool,
    /// The reason no mirror can be made. Without a usable git no git starts; short of disk, only clones wait.
    problem: Option<MirrorProblem>,
    /// The problem the window last heard of. A Check again forgets `problem` but keeps this, so the window
    /// hears only of a change.
    told: Option<MirrorProblem>,
    /// A copy of the user's answers of where each mirror gets its objects, as the clone requests bring them.
    sources: MirrorSources,
    /// The Repositories whose clone waits for an answer, in the order they came to the front of the line.
    asking: Vec<RepositoryName>,
    /// The first of `asking` the window last heard of.
    told_asking: Option<RepositoryName>,
    /// The times each Repository's mirror last had its upkeep, for those the worker has heard of.
    upkept: Vec<(RepositoryName, Upkept)>,
    /// The Repositories whose broken mirror the worker cloned again since launch.
    replaced: Vec<RepositoryName>,
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
                Ok(Message::Clone(repository, turn, source)) => self.wait(repository, turn, source),
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
                Ok(Message::Opened(repository, number)) => self.note_opened(&repository, number),
                Ok(Message::Hold(held)) => {
                    self.heard_news();
                    self.pace = if held { Pace::Held } else { Pace::Going };
                }
                Ok(Message::CheckAgain) => self.check_again(),
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
            Some(
                Job::Clone(Clone { mut git, .. })
                | Job::Fetch(Fetch { mut git, .. })
                | Job::Upkeep(Upkeep { mut git, .. }),
            ) => git.stop(),
            None => {}
        }
    }

    fn wait(&mut self, repository: RepositoryName, turn: Turn, source: Option<MirrorSource>) {
        if let Some(source) = source {
            self.sources.set(repository.clone(), source);
        }
        self.note_for_upkeep(&repository);
        if let Some(index) = self.asking.iter().position(|one| one.is_same(&repository)) {
            if self.sources.get(&repository).is_none() {
                return;
            }
            drop(self.asking.remove(index));
            self.tell_asking();
        }
        let cloning = match &self.job {
            Some(Job::Clone(clone)) => Some(&clone.cloning.repository),
            Some(Job::Fetch(_) | Job::Upkeep(_)) | None => None,
        };
        if cloning.is_some_and(|cloning| cloning.is_same(&repository)) {
            return;
        }
        let waiting = self.waiting.iter().position(|one| one.is_same(&repository));
        let failed = self
            .failed
            .iter()
            .position(|(one, _)| one.is_same(&repository));
        match turn {
            // A clone that failed goes back in line once its wait is over.
            Turn::Last => {
                if waiting.is_none() && failed.is_none() {
                    self.waiting.push_back(repository);
                }
            }
            // The user wants this one now, so its wait is over.
            Turn::Next => {
                if let Some(index) = waiting {
                    drop(self.waiting.remove(index));
                }
                if let Some(index) = failed {
                    drop(self.failed.remove(index));
                }
                self.waiting.push_front(repository);
            }
        }
    }

    fn finish_job(&mut self, job: Job, last_words: &str) {
        match job {
            Job::Clone(clone) => self.finish(clone, last_words),
            Job::Fetch(fetch) => self.finish_fetch(fetch, last_words),
            Job::Upkeep(upkeep) => self.finish_upkeep(upkeep, last_words),
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
                opened: false,
                handed: None,
                viewed: BTreeSet::new(),
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
        known.handed = Some(heads.iter().map(|head| head.number).collect());
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

    /// Keeps the ref of PR `number` in `repository`'s mirror until the next launch.
    fn note_opened(&mut self, repository: &RepositoryName, number: u32) {
        match (self.heads.iter_mut()).find(|known| known.repository.is_same(repository)) {
            Some(known) => drop(known.viewed.insert(number)),
            None => {
                let mut known = self.take_heads(repository);
                let _ = known.viewed.insert(number);
                self.heads.push_back(known);
            }
        }
    }

    /// Puts the head of a PR the user opened in the next fetch, whatever the worker noted of it, and its
    /// Repository ahead of the others.
    fn put_first(&mut self, repository: &RepositoryName, head: PrHead) {
        let mut known = self.take_heads(repository);
        drop(known.behind.remove(&head.number));
        drop(known.settled.remove(&head.number));
        drop(known.waiting.insert(head.number, head.commit));
        let _ = known.branches.insert(head.target_branch);
        let _ = known.viewed.insert(head.number);
        known.opened = true;
        self.heads.push_front(known);
    }

    /// Starts the next git, unless one is under way. A fetch goes ahead of a clone, as it takes a moment
    /// and a clone can take minutes, and both go ahead of a mirror's upkeep, which can wait. Nothing starts
    /// without a usable git, and short of disk only fetches and upkeep do.
    fn start_next(&mut self) {
        // Before the checks, so a wait that is over leaves `next_due` even while nothing can start.
        self.retry_due();
        self.retry_failed(Instant::now());
        if !self.git_checked {
            self.check_git();
        }
        if matches!(
            self.problem,
            Some(MirrorProblem::NoGit | MirrorProblem::OldGit { .. })
        ) {
            return;
        }
        self.start_fetch_or_clone();
        if self.job.is_none() {
            self.start_upkeep();
        }
    }

    /// Starts a fetch, or else a clone, unless GitHub can't be reached.
    fn start_fetch_or_clone(&mut self) {
        while self.job.is_none() && self.pace != Pace::Held {
            if let Some((repository, batch, mirror)) = self.next_batch() {
                self.start_fetch(repository, batch, mirror);
                continue;
            }
            if self.problem.is_some() {
                return;
            }
            let Some(repository) = self.waiting.pop_front() else {
                // A Check again found nothing left to clone, so nothing is short.
                self.set_problem(None);
                return;
            };
            if self.try_clone(repository).is_break() {
                return;
            }
        }
    }

    /// Starts a clone of `repository` unless it has its mirror, or can't have one yet. Breaks when the disk
    /// is short, which holds back the clones after it too.
    fn try_clone(&mut self, repository: RepositoryName) -> ControlFlow<()> {
        let folders = match &self.folders {
            Ok(folders) => folders,
            Err(problem) => {
                log::warn!(
                    "Tsugiki can't clone {repository}'s mirror: {}",
                    problem.message
                );
                return ControlFlow::Continue(());
            }
        };
        let folder = folders.repository(&repository);
        if folders.mirror(&repository).exists() {
            return ControlFlow::Continue(());
        }
        let Some(source) = self.sources.get(&repository).cloned() else {
            self.asking.push(repository);
            self.tell_asking();
            return ControlFlow::Continue(());
        };
        // What a clone cut off by quitting left goes first, or the disk would count it as taken.
        let partial = folder.join(PARTIAL);
        if partial.exists()
            && let Err(cause) = fs::remove_dir_all(&partial)
        {
            log::warn!(
                "Tsugiki can't delete the clone of {repository} that a quit cut off: {cause}"
            );
            self.fail_clone(repository);
            return ControlFlow::Continue(());
        }
        let root = folders.data.clone();
        match self.room_for(&repository, &root, &folders.mirror(&repository)) {
            Ok(None) => self.set_problem(None),
            Ok(Some(short)) => {
                self.waiting.push_front(repository);
                self.set_problem(Some(short));
                return ControlFlow::Break(());
            }
            Err(cause) => {
                log::warn!("Tsugiki can't say if {repository}'s mirror has room: {cause}");
                self.fail_clone(repository);
                return ControlFlow::Continue(());
            }
        }
        let reference = self.reference(&repository, &source);
        match self.start_clone(repository, folder, reference.as_deref()) {
            Ok(clone) => {
                (self.announce.cloning)(Some(&clone.cloning));
                self.job = Some(Job::Clone(clone));
            }
            Err((repository, cause)) => {
                log::warn!("Tsugiki can't start cloning {repository}'s mirror: {cause}");
                self.fail_clone(repository);
            }
        }
        ControlFlow::Continue(())
    }

    /// Runs `git --version`, and notes git as missing or too old if it is.
    fn check_git(&mut self) {
        self.git_checked = true;
        let problem = match self.git.read(&[OsStr::new("--version")]) {
            Ok(said) => match git_version(&said) {
                Some(version) if version >= OLDEST_GIT => None,
                Some(_) | None => Some(MirrorProblem::OldGit {
                    version: said.trim().to_owned(),
                }),
            },
            Err(cause) => {
                log::warn!("Tsugiki can't run git: {cause}");
                Some(MirrorProblem::NoGit)
            }
        };
        // A good git says nothing of a short disk the window heard of: the next clone does.
        let told_of_git = matches!(
            self.told,
            Some(MirrorProblem::NoGit | MirrorProblem::OldGit { .. })
        );
        if problem.is_some() || told_of_git {
            self.set_problem(problem);
        }
    }

    /// Forgets what the worker found of git and the disk, and puts the clones that failed back in line.
    fn check_again(&mut self) {
        self.git_checked = false;
        self.problem = None;
        let failed = mem::take(&mut self.failed);
        self.waiting
            .extend(failed.into_iter().map(|(repository, _)| repository));
    }

    /// Notes `problem`, and tells the window if it hasn't heard of it.
    fn set_problem(&mut self, problem: Option<MirrorProblem>) {
        if self.told != problem {
            (self.announce.problem)(problem.as_ref());
            self.told.clone_from(&problem);
        }
        self.problem = problem;
    }

    /// Says what a clone of `repository` at `mirror` lacks on the disk that holds `root`, if anything. A
    /// broken mirror waiting for this clone gives back its room when the disk is short.
    fn room_for(
        &mut self,
        repository: &RepositoryName,
        root: &Path,
        mirror: &Path,
    ) -> Result<Option<MirrorProblem>, String> {
        let size = (self.remotes.size_of)(repository).map_err(|problem| problem.message)?;
        let needed = size.saturating_add(ROOM_TO_SPARE);
        let mut free = self.free_space(root)?;
        if free < needed && delete_broken(repository, mirror) {
            free = self.free_space(root)?;
        }
        Ok((free < needed).then(|| MirrorProblem::DiskShort {
            repository: repository.clone(),
            folder: root.display().to_string(),
            needed_mb: megabytes(needed),
            free_mb: megabytes(free),
        }))
    }

    fn free_space(&self, root: &Path) -> Result<u64, String> {
        (self.remotes.free_space)(root).map_err(|cause| cause.to_string())
    }

    /// Tries a clone of `repository` again after a wait, which doubles each time it fails in a row.
    fn fail_clone(&mut self, repository: RepositoryName) {
        let tries = match self
            .clone_tries
            .iter_mut()
            .find(|(one, _)| one.is_same(&repository))
        {
            Some((_, tries)) => {
                *tries = tries.saturating_add(1);
                *tries
            }
            None => {
                self.clone_tries.push((repository.clone(), 1));
                1
            }
        };
        let at = Instant::now() + clone_wait(self.remotes.retry, tries);
        self.failed.push((repository, at));
    }

    /// Puts each clone that failed back in line once its wait is over.
    fn retry_failed(&mut self, now: Instant) {
        let (due, later) = mem::take(&mut self.failed)
            .into_iter()
            .partition(|(_, at)| *at <= now);
        self.failed = later;
        let due: Vec<(RepositoryName, Instant)> = due;
        self.waiting
            .extend(due.into_iter().map(|(repository, _)| repository));
    }

    /// The time the first head that is behind is due to be fetched once more, the first clone that failed
    /// is due to go back in line, or the first mirror's upkeep falls due.
    fn next_due(&self) -> Option<Instant> {
        let heads = self
            .heads
            .iter()
            .flat_map(|heads| heads.behind.values())
            .filter_map(|behind| match behind {
                Behind::Due { at, .. } => Some(*at),
                Behind::Retried { .. } => None,
            });
        let clones = self.failed.iter().map(|(_, at)| *at);
        // Only an idle worker with a usable git starts an upkeep, so only it wakes for one.
        let git_unusable = matches!(
            self.problem,
            Some(MirrorProblem::NoGit | MirrorProblem::OldGit { .. })
        );
        let upkeep = (self.job.is_none() && !git_unusable).then(|| self.upkeep_due());
        heads.chain(clones).chain(upkeep.flatten()).min()
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
        heads.opened = false;
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
            // A mirror cloned before Tsugiki kept it has no `maintenance.auto=false` of its own.
            OsStr::new("-c"),
            OsStr::new("maintenance.auto=false"),
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
                let numbers: Vec<u32> = batch.heads.keys().copied().collect();
                self.try_again_later(&repository, batch);
                self.pace = Pace::Stalled;
                let why = Unfetched::Git {
                    words: cause.to_string(),
                };
                (self.announce.failed)(&repository, &numbers, &why);
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
        self.requeue(known);
    }

    /// Puts a Repository's heads back after its fetch: behind the other Repositories', so each takes its
    /// turn, unless an opened PR's head waits.
    fn requeue(&mut self, known: Heads) {
        if known.opened {
            self.heads.push_front(known);
        } else {
            self.heads.push_back(known);
        }
    }

    /// Notes the heads a fetch brought, which no longer wait. A failed fetch leaves the refs it had, and its
    /// heads wait for the next fetch.
    ///
    /// GitHub moves `refs/pull/N/head` a moment after a push, so a fetch can bring the commit before the
    /// one handed over (#201). A head whose ref doesn't point at its commit is behind, and so is each head
    /// of a fetch whose refs can't be read back, and a head whose glob matched nothing, as GitHub has no
    /// ref for it. Each is fetched once more after a moment. If that fetch doesn't bring it either, the app
    /// hears why (#213), and a head with no ref is settled until it moves on.
    fn finish_fetch(&mut self, fetch: Fetch, last_words: &str) {
        let Fetch {
            repository,
            mirror,
            batch,
            mut git,
            news,
        } = fetch;
        if !git.succeeded() {
            let numbers: Vec<u32> = batch.heads.keys().copied().collect();
            self.try_again_later(&repository, batch);
            if says_broken(last_words) && self.replace_broken(&repository, &mirror, last_words) {
                return;
            }
            log::warn!("Tsugiki couldn't fetch PR heads of {repository}: {last_words}");
            // A fetch starts only while the worker is going, and no news since means it still is. Fetches
            // wait for some, so one that can't work doesn't run again and again. Only then does the app
            // hear of it, as news means another fetch.
            if !news {
                self.pace = Pace::Stalled;
                let why = Unfetched::Git {
                    words: last_words.to_owned(),
                };
                (self.announce.failed)(&repository, &numbers, &why);
            }
            return;
        }
        let landed = self.read_pull_refs(&mirror).map_err(|cause| {
            log::warn!("Tsugiki can't read which PR heads of {repository} it fetched: {cause}");
            cause.to_string()
        });
        let retry = Instant::now() + self.remotes.patience;
        let mut known = self.take_heads(&repository);
        let noted = known.note_fetched(batch.heads, landed.as_ref().map_err(String::as_str), retry);
        self.requeue(known);
        if !noted.arrived.is_empty() {
            (self.announce.fetched)(&repository, &noted.arrived);
        }
        for (why, numbers) in &noted.stuck {
            (self.announce.failed)(&repository, numbers, why);
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

    /// The git folder a clone of `repository` borrows objects from, if `source` names a clone that can lend
    /// them. One that can't any more downloads everything, and the log says why.
    fn reference(&self, repository: &RepositoryName, source: &MirrorSource) -> Option<PathBuf> {
        match source {
            MirrorSource::Download => None,
            MirrorSource::Borrow { folder } => {
                match lender(&*self.git, &self.remotes.prefixes, folder, repository) {
                    Ok(common) => Some(common),
                    Err(refusal) => {
                        let folder = folder.display();
                        log::warn!(
                            "Tsugiki downloads {repository}'s mirror, as {folder} can't lend it objects: {refusal:?}"
                        );
                        None
                    }
                }
            }
        }
    }

    /// Tells the window the first Repository whose clone waits for an answer, if that changed.
    fn tell_asking(&mut self) {
        let first = self.asking.first().cloned();
        let same = match (&first, &self.told_asking) {
            (Some(first), Some(told)) => first.is_same(told),
            (first, told) => first.is_none() && told.is_none(),
        };
        if !same {
            (self.announce.question)(first.as_ref());
            self.told_asking = first;
        }
    }

    fn start_clone(
        &self,
        repository: RepositoryName,
        folder: PathBuf,
        reference: Option<&Path>,
    ) -> Result<Clone, (RepositoryName, io::Error)> {
        let partial = folder.join(PARTIAL);
        let mirror = folder.join("mirror.git");
        let url = self.remotes.url(&repository);
        let started = fs::create_dir_all(&folder)
            .and_then(|()| self.start_clone_git(&url, &partial, reference));
        match started {
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

    /// Starts `git clone` into `partial`, borrowing what it can from the git folder `reference`.
    /// `--dissociate` copies what it borrowed, so the mirror doesn't lean on the user's clone afterwards.
    fn start_clone_git(
        &self,
        url: &str,
        partial: &Path,
        reference: Option<&Path>,
    ) -> io::Result<Started> {
        // The worker runs each mirror's upkeep itself, at a time of its choosing.
        let args = [
            "clone",
            "--bare",
            "--no-tags",
            "--progress",
            "--config",
            "maintenance.auto=false",
        ]
        .map(OsStr::new);
        let borrow = reference.map_or_else(Vec::new, |reference| {
            vec![
                OsStr::new("--reference"),
                reference.as_os_str(),
                OsStr::new("--dissociate"),
            ]
        });
        let target = [OsStr::new(url), partial.as_os_str()];
        self.git.start(&[&args[..], &borrow, &target].concat(), &[])
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

    /// Moves a finished clone to where a mirror goes, or deletes what a failed one left and tries it again
    /// later. git's last words say why it failed.
    fn finish(&mut self, mut clone: Clone, last_words: &str) {
        let repository = clone.cloning.repository.clone();
        let ended = if clone.git.succeeded() {
            fs::rename(&clone.partial, &clone.mirror)
        } else {
            Err(io::Error::other(last_words.to_owned()))
        };
        match ended {
            Ok(()) => {
                log::info!("Tsugiki cloned {repository}'s mirror");
                self.clone_tries
                    .retain(|(one, _)| !one.is_same(&repository));
                self.cloned_afresh(&repository, &clone.mirror);
            }
            Err(why) => {
                log::warn!("Tsugiki couldn't clone {repository}'s mirror: {why}");
                // What is left goes before the next clone of this Repository, which says if it can't.
                drop(fs::remove_dir_all(&clone.partial));
                self.fail_clone(repository);
            }
        }
        (self.announce.cloning)(None);
    }
}

/// The wait before a clone that failed `tries` times in a row goes back in line: `retry`, doubled for each
/// failure before the last, up to `MAX_DOUBLINGS` times.
fn clone_wait(retry: Duration, tries: u32) -> Duration {
    retry.saturating_mul(1 << tries.saturating_sub(1).min(MAX_DOUBLINGS))
}

/// The major and minor version in `git --version`'s answer, such as `git version 2.43.0.windows.1`.
fn git_version(said: &str) -> Option<(u32, u32)> {
    let version = said.trim().strip_prefix("git version ")?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// `bytes` in whole megabytes, at most `u32::MAX`, which the window shows as it is.
fn megabytes(bytes: u64) -> u32 {
    u32::try_from(bytes >> 20).unwrap_or(u32::MAX)
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
/// progress with a carriage return, and other lines with a newline. Its last words are its last `fatal:`
/// or `error:` line, as hints follow the reason, or else its last line.
fn read_progress_until_done(progress: Box<dyn Read + Send>, messages: &Sender<Message>) {
    let mut last_words = String::new();
    let mut last_error = String::new();
    let mut line = Vec::new();
    for byte in BufReader::new(progress).bytes().map_while(Result::ok) {
        if byte != b'\r' && byte != b'\n' {
            line.push(byte);
            continue;
        }
        let text = String::from_utf8_lossy(&line);
        match read_progress(&text) {
            Some(progress) => drop(messages.send(Message::Progress(progress))),
            None if text.starts_with("fatal: ") || text.starts_with("error: ") => {
                last_error = text.trim().to_owned();
            }
            None if !text.trim().is_empty() => last_words = text.trim().to_owned(),
            None => {}
        }
        line.clear();
    }
    if !last_error.is_empty() {
        last_words = last_error;
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
