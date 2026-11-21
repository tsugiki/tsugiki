//! Each Repository's **Mirror**: a bare clone in its folder (ADR 0002, ADR 0010). One worker thread writes
//! to the mirrors, beside the session's sync queue, and clones them one at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufReader, Read};
use std::mem;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::CommitSha;
use crate::facts::patch_ids::{FilePatch, PatchesOf};
use crate::facts::pull_requests::{PrHead, ReviewedCommit};
use crate::folders::Folders;
use crate::github::live;
use crate::latest::{self, LatestReader, LatestWriter};
use crate::mirror_queue::{self, Next, Pace, Problem, Queue, Queued, Waiting};
use crate::mirror_source::{MirrorSource, MirrorSources};
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::status::Status;

mod heads;
mod lookup;
mod patches;
mod reviewed;
mod source;
mod upkeep;
use heads::{Prs, Retry};
use lookup::{Lookup, Move};
pub use lookup::{LookupAt, LookupCheckout};
use patches::Patches;
use reviewed::{KeepReviewed, Reviews};
pub use source::CloneRefusal;
use source::{GITHUB_PREFIXES, lender};
use upkeep::{Schedule, Upkeep, Upkept, delete_broken, has_broken, says_broken};

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

impl MirrorProblem {
    /// The jobs the problem holds back: every one without a usable git. Short of disk, the clones, and a
    /// move that would make its Lookup checkout afresh, which the move refuses itself as it answers the
    /// readers who wait for it.
    fn holds_back(&self) -> Problem {
        match self {
            Self::NoGit | Self::OldGit { .. } => Problem::Git,
            Self::DiskShort { .. } => Problem::DiskShort,
        }
    }

    /// Whether the problem leaves no usable git.
    fn leaves_no_git(&self) -> bool {
        self.holds_back() == Problem::Git
    }
}

/// How far a Repository's mirror is from ready, so the Repository picker can say why its diffs aren't there
/// yet (#320).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MirrorState {
    /// The mirror is there.
    Ready,
    /// Its clone is under way.
    Cloning { progress: CloneProgress },
    /// Its clone waits for the user to say where it gets its objects.
    Asking,
    /// Its clone waits in line behind the clone of `behind`. With no `behind`, no clone is under way, and a
    /// fetch, the keeping of reviewed commits or a mirror's upkeep goes first.
    Waiting { behind: Option<RepositoryName> },
    /// Its clone waits for GitHub, which can't be reached, with no clone under way ahead of it. It starts
    /// once Tsugiki is back online (#390).
    Offline,
    /// Its last clone failed, for the reason in `words`, and it goes back in line after a wait.
    Failed { words: String },
    /// `problem` keeps its clone from starting. A disk short of room for one clone holds back the others
    /// too, and names the one it is short for.
    Problem { problem: MirrorProblem },
}

/// The state of one Repository's mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct RepositoryMirror {
    pub repository: RepositoryName,
    pub state: MirrorState,
}

/// The oldest git a mirror works with (ADR 0003): `git check-attr --source` and the partial clone's
/// options came in 2.30 (#166).
const OLDEST_GIT: (u32, u32) = (2, 30);

/// The oldest git the Stale review check works with: `git patch-id --verbatim` came in 2.39 (#384).
const CHECK_GIT: (u32, u32) = (2, 39);

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

/// Where the worker learns a Repository's size on GitHub.
enum Sizes {
    /// What the remotes were made with, as in tests.
    Own(SizeOf),
    /// The session's GitHub thread, which owns GitHub access, once the session hands over the way to ask it
    /// (`MirrorRequests::size_through`). The rate limit's gate then holds the request as it holds every
    /// other.
    FromSession(Option<SizeOf>),
}

impl Sizes {
    fn size_of(&mut self, repository: &RepositoryName) -> Result<u64, Error> {
        match self {
            Self::Own(size_of) | Self::FromSession(Some(size_of)) => size_of(repository),
            Self::FromSession(None) => Err(Error::new(
                ErrorKind::Offline,
                &format!("Tsugiki can't ask GitHub for the size of {repository} before it connects."),
            )),
        }
    }

    /// Asks the session through `size_of` from now on, if the remotes are GitHub's.
    fn take(&mut self, size_of: SizeOf) {
        if let Self::FromSession(asking) = self {
            *asking = Some(size_of);
        }
    }
}

/// Where git clones to, in the Repository's folder, so a clone cut off never looks like a mirror.
const PARTIAL: &str = "mirror.git.partial";

/// A file in `PARTIAL` that says git finished that clone, so the clone is kept until it can be moved to the
/// mirror's place (#194). Without it, the next clone deletes `PARTIAL` as cut off.
const CLONED: &str = "tsugiki-cloned";

/// Says the clone under way each time it moves on, and `None` when it ends.
pub type AnnounceCloning = Box<dyn FnMut(Option<&Cloning>) + Send>;

/// Says which PRs' heads a fetch brought into a Repository's mirror.
pub type AnnounceFetched = Box<dyn FnMut(&RepositoryName, &[u32]) + Send>;

/// Says which PRs' heads a fetch into a Repository's mirror failed to bring, and why, once the worker
/// stopped fetching them: a failed fetch waits for news, and a head that didn't land at its retry waits for
/// a hand-over, or for its head to move on if GitHub has no ref for it.
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

/// Says the state of the mirror of each Repository the worker has heard of, in the order it heard of them,
/// each time one changes.
pub type AnnounceStates = Box<dyn FnMut(&[RepositoryMirror]) + Send>;

/// Says whether the worker holds back git while GitHub can't be reached, each time that changes, so the
/// Files tab can say that the PR's commits wait for the network (#390).
pub type AnnounceHeld = Box<dyn FnMut(bool) + Send>;

/// Hands over the per-file patch-ids the worker read of commits in a Repository's mirror (#383), and the
/// commits whose patch-ids it can't read now and why (#384), for the session to keep. The session starts
/// after the worker, so `MirrorRequests::hand_patches_to` gives it.
pub type AnnouncePatched = Box<dyn FnMut(&RepositoryName, PatchReads) + Send>;

/// What the worker tells the app as it goes.
pub struct Announce {
    pub cloning: AnnounceCloning,
    pub fetched: AnnounceFetched,
    pub failed: AnnounceFailed,
    pub problem: AnnounceProblem,
    pub question: AnnounceQuestion,
    pub states: AnnounceStates,
    pub held: AnnounceHeld,
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
            states: Box::new(|_| {}),
            held: Box::new(|_| {}),
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
    size_of: Sizes,
    free_space: FreeSpace,
    /// The schedule of each mirror's upkeep, or nothing for mirrors left as they are.
    upkeep: Option<Schedule>,
}

impl Remotes {
    /// GitHub's remotes, whose sizes its API gives through the session.
    pub fn github() -> Self {
        Self {
            size_of: Sizes::FromSession(None),
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
            size_of: Sizes::Own(Box::new(|_| Ok(0))),
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
            size_of: Sizes::Own(size_of),
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
    /// The heads of the PRs in a Repository's Reach, as the store has them now.
    Heads(RepositoryName, Vec<PrHead>),
    /// The commits the user's Reviews in a Repository covered, as the store has them now.
    Reviewed(RepositoryName, Vec<ReviewedCommit>),
    /// The commits in a Repository whose patch-ids the Stale review check needs and the store lacks.
    Patches(RepositoryName, Vec<PatchesOf>),
    /// Where to hand the patch-ids read from now on.
    PatchesTo(AnnouncePatched),
    /// How to ask GitHub a Repository's size from now on.
    SizesFrom(SizeOf),
    /// The head of a PR the user opened, which the mirror lacks.
    First(RepositoryName, PrHead),
    /// A PR the user opened, whose ref stays until the next launch.
    Opened(RepositoryName, u32),
    /// A PR the user opened before the session knew its head, whose head goes first once it does.
    Awaited(RepositoryName, u32),
    /// A PR whose head no read will name, so its await ends.
    Unawaited(RepositoryName, u32),
    /// Heads of a Repository's PRs that a sync step saved.
    Saved(RepositoryName, Vec<PrHead>),
    /// Where the PR on screen wants a Repository's Lookup checkout.
    LookUp(RepositoryName, LookupAt),
    /// A reader asks where a Repository's Lookup checkout is, once it has stopped moving.
    Checkout(RepositoryName, Sender<Option<LookupCheckout>>),
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
    /// A test asks to hear once the worker has handled the messages before this one and has nothing to do
    /// at once, so it can say that no other git started.
    #[cfg(test)]
    Idle(Sender<()>),
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
    asking: LatestReader<Vec<RepositoryName>>,
    /// The worker's thread, which a test joins to learn how it ended. Quitting doesn't join it, so a worker
    /// stuck on a disk can't hold the app up.
    #[cfg_attr(not(test), expect(dead_code, reason = "only a test joins the worker"))]
    worker: Option<thread::JoinHandle<()>>,
}

/// A way to ask for mirrors. It returns at once, but for `check_clone`.
#[derive(Clone)]
pub struct MirrorRequests {
    messages: Sender<Message>,
    /// The remotes' prefixes, so a clone the user picks is checked as the worker will check it.
    prefixes: Vec<String>,
    /// The worker's copy of the Repositories whose clone waits for an answer.
    asking: LatestReader<Vec<RepositoryName>>,
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

    /// Whether `repository`'s clone waits for the user to say where its mirror gets its objects.
    pub fn asks(&self, repository: &RepositoryName) -> bool {
        (self.asking.get().iter()).any(|one| one.is_same(repository))
    }

    /// Fetches the PR heads in `heads` into `repository`'s mirror, as `refs/tsugiki/pull/N/head`, once
    /// its mirror is there. Heads handed over while a fetch is under way go in the next one, all in one.
    /// `heads` is the whole set the rules want, so it replaces the last: a head that left it and is still
    /// waiting isn't fetched. It leaves the head of a PR the user opened as it was.
    pub fn fetch_heads(&self, repository: RepositoryName, heads: Vec<PrHead>) {
        self.send(Message::Heads(repository, heads));
    }

    /// Keeps each commit in `reviewed` under `refs/tsugiki/reviewed/N` in `repository`'s mirror, once its
    /// mirror is there, after the heads waiting. A commit the mirror lacks is fetched by its SHA, once: one
    /// GitHub refuses to send is noted beside the mirror and not asked for again.
    pub fn keep_reviewed(&self, repository: RepositoryName, reviewed: Vec<ReviewedCommit>) {
        self.send(Message::Reviewed(repository, reviewed));
    }

    /// Reads the per-file patch-ids of each of `wanted` in `repository`'s mirror, once the mirror has the
    /// commit, after the heads and reviewed commits waiting, and hands them to what `hand_patches_to` gave.
    /// A commit read since launch isn't read again. The worker also hands over which of `wanted` it can't
    /// read and why, each time that changes.
    pub fn read_patch_ids(&self, repository: RepositoryName, wanted: Vec<PatchesOf>) {
        self.send(Message::Patches(repository, wanted));
    }

    /// Hands the patch-ids the worker reads to `announce` from now on. Until then the worker reads none, so
    /// none are lost.
    pub fn hand_patches_to(&self, announce: AnnouncePatched) {
        self.send(Message::PatchesTo(announce));
    }

    /// Asks a Repository's size before a clone through `size_of` from now on, the session's way to its
    /// GitHub thread, if the worker clones from GitHub. Until then a clone fails its size check, and tries
    /// again after its wait. A session with no token answers each ask with a failure, so its clones keep
    /// failing until the next launch.
    pub fn size_through(&self, size_of: SizeOf) {
        self.send(Message::SizesFrom(size_of));
    }

    /// Fetches the head of a PR the user opened, which `repository`'s mirror lacks, ahead of the heads
    /// waiting, in a fetch of its own, or in the fetch of that PR at the same commit under way. The worker
    /// fetches it whatever it noted of it before, so a ref GitHub made late arrives. A hand-over never
    /// replaces it, and the app hears it is behind only from the fetches it was in.
    pub fn fetch_first(&self, repository: RepositoryName, head: PrHead) {
        self.send(Message::First(repository, head));
    }

    /// Keeps the ref of a PR the user opened until the next launch, though the user's work may not reach it.
    pub fn opened(&self, repository: RepositoryName, number: u32) {
        self.send(Message::Opened(repository, number));
    }

    /// Keeps the ref of PR `number`, which the user opened before the session knew its head, as `opened`
    /// does, and fetches its head as `fetch_first` does once `heads_saved` names it.
    pub fn await_head(&self, repository: RepositoryName, number: u32) {
        self.send(Message::Awaited(repository, number));
    }

    /// Ends the await of PR `number`, whose Conversation named no head or couldn't be read, so a head a sync
    /// step saves later takes its turn with the others. Its ref stays until the next launch.
    pub fn no_head(&self, repository: RepositoryName, number: u32) {
        self.send(Message::Unawaited(repository, number));
    }

    /// Says a sync step saved `heads` of `repository`'s PRs. The head of a PR `await_head` named goes first,
    /// and the worker passes over the others.
    pub fn heads_saved(&self, repository: RepositoryName, heads: Vec<PrHead>) {
        self.send(Message::Saved(repository, heads));
    }

    /// Moves `repository`'s **Lookup checkout** to `at`, the PR on screen, in the background. The checkout
    /// is made on the first call.
    pub fn look_up(&self, repository: RepositoryName, at: LookupAt) {
        self.send(Message::LookUp(repository, at));
    }

    /// Where `repository`'s Lookup checkout is, once the moves under way and due have ended: so a lookup
    /// asked during a move reads the new commit. None when no move has put it anywhere since launch, the
    /// last one failed, or the worker has stopped.
    pub fn lookup_checkout(&self, repository: RepositoryName) -> Option<LookupCheckout> {
        self.ask_lookup_checkout(repository).recv().ok().flatten()
    }

    /// The same, at once: the answer comes on what it returns, so the caller can say that it waits.
    pub fn ask_lookup_checkout(
        &self,
        repository: RepositoryName,
    ) -> Receiver<Option<LookupCheckout>> {
        let (reply, answer) = mpsc::channel();
        self.send(Message::Checkout(repository, reply));
        answer
    }

    /// Looks at git and the disk again, and tries each clone that waits for them or failed.
    pub fn check_again(&self) {
        self.send(Message::CheckAgain);
    }

    /// Holds back the next git while `status` says GitHub can't be reached, or hasn't answered since it
    /// couldn't be. A rate limit is the API's, and git doesn't use the API, so it holds back nothing.
    pub fn follow(&self, status: &Status) {
        self.send(Message::Hold(status.is_offline()));
    }

    fn send(&self, message: Message) {
        if self.messages.send(message).is_err() {
            log::warn!("Tsugiki has stopped working on mirrors, so it can't take another request");
        }
    }
}

/// Requests that reach a test, which hears what each asks, and no worker. No clone waits for an
/// answer.
#[cfg(test)]
pub(crate) struct HeardRequests {
    messages: Receiver<Message>,
    /// What the test hasn't asked for yet, of what it heard.
    unasked: std::cell::RefCell<Vec<Message>>,
}

#[cfg(test)]
impl HeardRequests {
    pub(crate) fn new() -> (MirrorRequests, Self) {
        let (messages, heard) = mpsc::channel();
        let (_asked, asking) = latest::channel(Vec::new());
        let requests = MirrorRequests {
            messages,
            prefixes: Vec::new(),
            asking,
        };
        let heard = Self {
            messages: heard,
            unasked: std::cell::RefCell::default(),
        };
        (requests, heard)
    }

    /// The Repositories whose clones were asked for since the last call.
    pub(crate) fn clones(&self) -> Vec<RepositoryName> {
        self.take(|message| {
            let Message::Clone(repository, ..) = message else {
                return Err(message);
            };
            Ok(repository)
        })
    }

    /// The way to ask a Repository's size that the session handed the worker, waiting up to `patience` for
    /// it.
    pub(crate) fn sizes(&self, patience: Duration) -> SizeOf {
        let deadline = Instant::now() + patience;
        loop {
            let handed = self.take(|message| {
                let Message::SizesFrom(size_of) = message else {
                    return Err(message);
                };
                Ok(size_of)
            });
            if let Some(size_of) = handed.into_iter().next() {
                return size_of;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let message = self.messages.recv_timeout(left).expect("a way to ask sizes");
            self.unasked.borrow_mut().push(message);
        }
    }

    /// The heads asked for first since the last call.
    pub(crate) fn firsts(&self) -> Vec<(RepositoryName, PrHead)> {
        self.take(|message| {
            let Message::First(repository, head) = message else {
                return Err(message);
            };
            Ok((repository, head))
        })
    }

    /// What was asked since the last call of what the rules want, the heads saved and the PRs opened, each in
    /// words, in the order asked.
    pub(crate) fn asked(&self) -> Vec<String> {
        fn listed(items: impl Iterator<Item = String>) -> String {
            let items: Vec<String> = items.collect();
            if items.is_empty() {
                "none".to_owned()
            } else {
                items.join(", ")
            }
        }
        let short = |commit: &CommitSha| commit.as_str().chars().take(7).collect::<String>();
        let heads = |heads: &[PrHead]| {
            listed(heads.iter().map(|head| {
                let PrHead {
                    number,
                    commit,
                    target_branch,
                } = head;
                format!("#{number} at {} on {target_branch}", short(commit))
            }))
        };
        self.take(|message| match message {
            Message::Heads(repository, wanted) => {
                Ok(format!("{repository} wants heads {}", heads(&wanted)))
            }
            Message::Reviewed(repository, reviewed) => {
                let reviewed = (reviewed.iter())
                    .map(|one| format!("#{} at {}", one.number, short(&one.commit)));
                Ok(format!("{repository} wants reviewed {}", listed(reviewed)))
            }
            Message::Patches(repository, wanted) => {
                let wanted = (wanted.iter())
                    .map(|of| format!("{} on {}", short(&of.commit), of.target_branch));
                Ok(format!(
                    "{repository} wants patch-ids of {}",
                    listed(wanted)
                ))
            }
            Message::Saved(repository, saved) => {
                Ok(format!("{repository} saved heads {}", heads(&saved)))
            }
            Message::Opened(repository, number) => Ok(format!("{repository} opened #{number}")),
            Message::Awaited(repository, number) => Ok(format!("{repository} awaits #{number}")),
            Message::Unawaited(repository, number) => {
                Ok(format!("{repository} awaits no head of #{number}"))
            }
            other @ (Message::Clone(..)
            | Message::PatchesTo(_)
            | Message::SizesFrom(_)
            | Message::First(..)
            | Message::LookUp(..)
            | Message::Checkout(..)
            | Message::Hold(_)
            | Message::CheckAgain
            | Message::Progress(_)
            | Message::Ended { .. }
            | Message::Idle(_)
            | Message::Stop) => Err(other),
        })
    }

    /// What `asked` gives, waiting up to `patience` for a line that holds `last`, after which a test has
    /// heard all it waits for.
    pub(crate) fn asked_through(&self, last: &str, patience: Duration) -> Vec<String> {
        let deadline = Instant::now() + patience;
        let left = || deadline.saturating_duration_since(Instant::now());
        let mut heard = self.asked();
        while !heard.iter().any(|line| line.contains(last))
            && let Ok(message) = self.messages.recv_timeout(left())
        {
            self.unasked.borrow_mut().push(message);
            heard.extend(self.asked());
        }
        heard
    }

    /// The moves of Lookup checkouts asked for since the last call.
    pub(crate) fn look_ups(&self) -> Vec<(RepositoryName, LookupAt)> {
        self.take(|message| {
            let Message::LookUp(repository, at) = message else {
                return Err(message);
            };
            Ok((repository, at))
        })
    }

    /// The questions of where a Lookup checkout is, asked since the last call, each with where to answer.
    pub(crate) fn checkouts(&self) -> Vec<(RepositoryName, Sender<Option<LookupCheckout>>)> {
        self.take(|message| {
            let Message::Checkout(repository, reply) = message else {
                return Err(message);
            };
            Ok((repository, reply))
        })
    }

    /// What `pick` takes of the messages heard, which keeps the others for the next call.
    fn take<T>(&self, pick: impl Fn(Message) -> Result<T, Message>) -> Vec<T> {
        let mut unasked = self.unasked.borrow_mut();
        let heard = unasked
            .drain(..)
            .chain(self.messages.try_iter())
            .collect::<Vec<_>>();
        let mut taken = Vec::new();
        for message in heard {
            match pick(message) {
                Ok(picked) => taken.push(picked),
                Err(other) => unasked.push(other),
            }
        }
        taken
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
        let (asked, asking) = latest::channel(Vec::new());
        let worker = Worker {
            git,
            folders,
            remotes,
            announce,
            patched: None,
            messages: messages.clone(),
            waiting: VecDeque::new(),
            failed: Vec::new(),
            clone_tries: Vec::new(),
            heads: VecDeque::new(),
            pace: Pace::Going,
            job: None,
            git_checked: false,
            problem: None,
            old_for_check: None,
            told: None,
            sources: MirrorSources::default(),
            asking: Vec::new(),
            asked,
            told_asking: None,
            heard: Vec::new(),
            told_states: Vec::new(),
            upkept: Vec::new(),
            replaced: Vec::new(),
            #[cfg(test)]
            awaiting_idle: Vec::new(),
        };
        let worker = thread::spawn(move || {
            // `stop` learns the worker is over when this goes, however the thread ends. Locals go in reverse
            // order, so `incoming` goes first, and a request after `stop` finds nobody to take it. The
            // worker's git goes before `stop` returns.
            let _finishing = finishing;
            let mut worker = worker;
            let incoming = incoming;
            worker.run(&incoming);
        });
        Self {
            messages,
            finished,
            prefixes,
            asking,
            worker: Some(worker),
        }
    }

    pub fn requests(&self) -> MirrorRequests {
        MirrorRequests {
            messages: self.messages.clone(),
            prefixes: self.prefixes.clone(),
            asking: self.asking.clone(),
        }
    }

    /// Stops a clone under way, with the helpers git started, and gives the worker up to `patience` to end.
    /// The next launch starts that clone again from an empty folder, unless git had finished it.
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

    /// Stops the mirrors as `stop` does, and fails the test unless the worker ended within `patience`
    /// without a panic. A panic there goes on in the test, with the worker's message.
    #[cfg(test)]
    pub(crate) fn stop_cleanly(mut self, patience: Duration) {
        self.begin_stopping();
        if let Err(RecvTimeoutError::Timeout) = self.finished.recv_timeout(patience) {
            panic!("the mirror worker didn't end within {patience:?} of hearing to stop");
        }
        let worker = self.worker.take().expect("only this joins the worker");
        if let Err(panic) = worker.join() {
            std::panic::resume_unwind(panic);
        }
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
    /// Reads the patch-ids of each file `of` changes in `mirror`, with the several gits that takes.
    fn read_patch_ids(&self, mirror: &Path, of: &PatchesOf) -> io::Result<Vec<FilePatch>>;
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
    /// Ends git and the helpers it started, and says if git had ended before the stop, having done what it
    /// was asked.
    fn stop_finished(&mut self) -> bool;
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

    fn read_patch_ids(&self, mirror: &Path, of: &PatchesOf) -> io::Result<Vec<FilePatch>> {
        crate::diff::read_patch_ids(mirror, &of.target_branch, &of.commit)
    }
}

impl Run for live::StartedGit {
    fn succeeded(&mut self) -> bool {
        Self::succeeded(self)
    }

    fn stop_finished(&mut self) -> bool {
        Self::stop_finished(self)
    }
}

/// The one git under way.
enum Job {
    Clone(Clone),
    Fetch(Fetch),
    Reviewed(KeepReviewed),
    Upkeep(Upkeep),
    Lookup(Move),
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
    /// Each head it fetches, at the commit asked for, with the Target branch it brings up to date.
    batch: Vec<PrHead>,
    /// When it started, which says of a head that joined it whether its moment was over.
    started: Instant,
    git: Box<dyn Run>,
    /// Heads or a status came while it ran, so if it fails, the next fetch has a reason to try again.
    news: bool,
}

/// How long a head whose fetch brought another commit waits before it is fetched once more. GitHub moves
/// `refs/pull/N/head` a few seconds after a push.
const BEHIND_PATIENCE: Duration = Duration::from_secs(10);

/// What the worker knows of one Repository's PR heads.
struct Heads {
    repository: RepositoryName,
    /// Where each PR's ref stands, the head the rules want and the head the user asked for.
    prs: Prs,
    /// The commits the user's Reviews covered.
    reviews: Reviews,
    /// The commits whose patch-ids the Stale review check needs.
    patches: Patches,
    /// The Repository's Lookup checkout.
    lookup: Lookup,
}

struct Worker {
    git: Box<dyn Git>,
    folders: Result<Folders, Error>,
    remotes: Remotes,
    announce: Announce,
    /// Where the patch-ids read go, once the session is there to keep them.
    patched: Option<AnnouncePatched>,
    /// For the thread that reads each git's progress.
    messages: Sender<Message>,
    waiting: VecDeque<RepositoryName>,
    /// The Repositories whose clone failed. A Repository is in one of `waiting`, `failed`, `asking` and
    /// the clone under way at most.
    failed: Vec<FailedClone>,
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
    /// What `git --version` says of a git that works with the mirrors but is older than the Stale review
    /// check needs. The worker reads no patch-ids with it.
    old_for_check: Option<String>,
    /// The problem the window last heard of. A Check again forgets `problem` but keeps this, so the window
    /// hears only of a change.
    told: Option<MirrorProblem>,
    /// A copy of the user's answers of where each mirror gets its objects, as the clone requests bring them.
    sources: MirrorSources,
    /// The Repositories whose clone waits for an answer, in the order they came to the front of the line.
    asking: Vec<RepositoryName>,
    /// The copy of `asking` the requests read.
    asked: LatestWriter<Vec<RepositoryName>>,
    /// The first of `asking` the window last heard of.
    told_asking: Option<RepositoryName>,
    /// The Repositories the worker has heard of, in the order it heard of them. Each one in none of
    /// `waiting`, `failed`, `asking` and the clone under way counts as ready: the worker cloned its mirror
    /// or found it there. A broken one it can't move aside, or one deleted behind its back, still counts.
    heard: Vec<RepositoryName>,
    /// The states of their mirrors the window last heard of.
    told_states: Vec<RepositoryMirror>,
    /// The times each Repository's mirror last had its upkeep, for those the worker has heard of.
    upkept: Vec<(RepositoryName, Upkept)>,
    /// The Repositories whose broken mirror the worker cloned again since launch.
    replaced: Vec<RepositoryName>,
    /// The tests that wait to hear the worker is idle.
    #[cfg(test)]
    awaiting_idle: Vec<Sender<()>>,
}

/// Why the worker can't say whether a clone has room on the disk.
#[derive(Debug, PartialEq, Eq)]
enum Unmeasured {
    /// The rate limit's gate holds every request to GitHub until `back`, so the size can't be asked before
    /// then, and the clone waits for it rather than for its doubled retry. `words` say why.
    PointsOut { back: Instant, words: String },
    /// Anything else, in words.
    Other(String),
}

impl Unmeasured {
    /// Why the size check failed, from its `problem`. A rate limit that names no reset fails like any other.
    fn of(problem: Error) -> Self {
        let ErrorKind::RateLimited { resets_at } = problem.kind else {
            return Self::Other(problem.message);
        };
        let back = resets_at
            .and_then(|at| OffsetDateTime::from_unix_timestamp(at.into()).ok())
            .map(|at| Duration::try_from(at - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO));
        match back {
            Some(left) => Self::PointsOut {
                back: Instant::now() + left,
                words: problem.message,
            },
            None => Self::Other(problem.message),
        }
    }
}

/// A Repository whose clone failed.
struct FailedClone {
    repository: RepositoryName,
    /// When it goes back in line.
    retry_at: Instant,
    /// Why it failed: git's last words, or what kept git from starting or the clone from being moved.
    words: String,
}

impl Worker {
    fn run(&mut self, incoming: &Receiver<Message>) {
        loop {
            // One time for the turn, so a head whose moment ends after the pick wakes the next turn.
            let now = Instant::now();
            // At once, after the messages that came meanwhile.
            let pick_again = self.start_next(now).then(Instant::now);
            self.tell_states();
            self.tell_unchecked();
            let due = self.next_due(pick_again, now);
            #[cfg(test)]
            self.tell_idle(due);
            let message = match due {
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
                    self.wake();
                    self.hand_over(&repository, heads);
                }
                Ok(Message::Reviewed(repository, reviewed)) => {
                    self.wake();
                    self.hand_over_reviewed(&repository, reviewed);
                }
                Ok(Message::Patches(repository, wanted)) => {
                    self.hand_over_patches(&repository, wanted);
                }
                Ok(Message::PatchesTo(announce)) => self.patched = Some(announce),
                Ok(Message::SizesFrom(size_of)) => self.remotes.size_of.take(size_of),
                Ok(Message::First(repository, head)) => {
                    self.wake();
                    self.put_first(&repository, head);
                }
                Ok(Message::Opened(repository, number)) => {
                    self.change_heads(&repository, |known| known.prs.opened(number));
                }
                Ok(Message::Awaited(repository, number)) => {
                    self.change_heads(&repository, |known| known.prs.await_head(number));
                }
                Ok(Message::Unawaited(repository, number)) => {
                    self.change_heads(&repository, |known| known.prs.no_head(number));
                }
                Ok(Message::Saved(repository, heads)) => self.saved(&repository, heads),
                Ok(Message::LookUp(repository, at)) => self.look_up(&repository, at),
                Ok(Message::Checkout(repository, reply)) => {
                    self.lookup_checkout(&repository, reply)
                }
                Ok(Message::Hold(held)) => {
                    self.heard_news();
                    if held != (self.pace == Pace::Held) {
                        (self.announce.held)(held);
                    }
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
                #[cfg(test)]
                Ok(Message::Idle(reply)) => self.awaiting_idle.push(reply),
                Ok(Message::Stop) | Err(_) => break,
            }
        }
        let Some(job) = self.job.take() else {
            return;
        };
        let (mut git, cloned) = match job {
            // The stop can come as git ends having cloned it all, and the next launch moves that clone.
            Job::Clone(Clone {
                git,
                cloning,
                partial,
                ..
            }) => (git, Some((cloning.repository, partial))),
            Job::Fetch(Fetch { git, .. })
            | Job::Reviewed(KeepReviewed { git, .. })
            | Job::Upkeep(Upkeep { git, .. })
            | Job::Lookup(Move { git, .. }) => (git, None),
        };
        // The mark goes before the wait for helpers, which can outlast the time quitting gives.
        if git.stop_finished()
            && let Some((repository, partial)) = cloned
        {
            mark_cloned(&repository, &partial);
        }
        wait_for_helpers(incoming);
    }

    fn wait(&mut self, repository: RepositoryName, turn: Turn, source: Option<MirrorSource>) {
        if let Some(source) = source {
            self.sources.set(repository.clone(), source);
        }
        self.note_for_upkeep(&repository);
        // Without its folder, nothing can be said of a mirror, and the clone logs why it can't start.
        if let Ok(folders) = &self.folders {
            if !self.heard.iter().any(|one| one.is_same(&repository)) {
                self.heard.push(repository.clone());
            }
            // So a mirror that is there doesn't look as if it waits for its clone.
            if folders.mirror(&repository).exists() {
                return;
            }
        }
        if self.asking.iter().any(|one| one.is_same(&repository)) {
            if self.sources.get(&repository).is_none() {
                return;
            }
            self.asking.retain(|one| !one.is_same(&repository));
            self.tell_asking();
        }
        let cloning = match &self.job {
            Some(Job::Clone(clone)) => Some(&clone.cloning.repository),
            Some(Job::Fetch(_) | Job::Reviewed(_) | Job::Upkeep(_) | Job::Lookup(_)) | None => None,
        };
        if cloning.is_some_and(|cloning| cloning.is_same(&repository)) {
            return;
        }
        let waits = |one: &RepositoryName| one.is_same(&repository);
        let failed = |failed: &FailedClone| failed.repository.is_same(&repository);
        match turn {
            // A clone that failed goes back in line once its wait is over.
            Turn::Last => {
                if !self.waiting.iter().any(waits) && !self.failed.iter().any(failed) {
                    self.waiting.push_back(repository);
                }
            }
            // The user wants this one now, so its wait is over.
            Turn::Next => {
                self.waiting.retain(|one| !waits(one));
                self.failed.retain(|one| !failed(one));
                self.waiting.push_front(repository);
            }
        }
    }

    fn finish_job(&mut self, job: Job, last_words: &str) {
        match job {
            Job::Clone(clone) => self.finish(clone, last_words),
            Job::Fetch(fetch) => self.finish_fetch(fetch, last_words),
            Job::Reviewed(keeping) => self.finish_reviewed(keeping, last_words),
            Job::Upkeep(upkeep) => self.finish_upkeep(upkeep, last_words),
            Job::Lookup(moving) => self.finish_lookup_move(moving, last_words),
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
                prs: Prs::default(),
                reviews: Reviews::default(),
                patches: Patches::default(),
                lookup: Lookup::default(),
            })
    }

    /// Changes what the worker knows of `repository`'s heads with `change`. The Repository keeps its turn
    /// for heads.
    fn change_heads(&mut self, repository: &RepositoryName, change: impl FnOnce(&mut Heads)) {
        let at = (self.heads.iter()).position(|known| known.repository.is_same(repository));
        let at = at.unwrap_or(self.heads.len());
        let mut known = self.take_heads(repository);
        change(&mut known);
        self.heads.insert(at, known);
    }

    /// Notes news for the git under way, and lets a stalled worker go on: something more to fetch came.
    fn wake(&mut self) {
        self.heard_news();
        if self.pace == Pace::Stalled {
            self.pace = Pace::Going;
        }
    }

    fn heard_news(&mut self) {
        match &mut self.job {
            Some(Job::Fetch(fetch)) => fetch.news = true,
            Some(Job::Reviewed(keeping)) => keeping.news = true,
            Some(Job::Clone(_) | Job::Upkeep(_) | Job::Lookup(_)) | None => {}
        }
    }

    /// Takes `heads` as the rules' want of `repository`, which replaces the last, and puts the Repository
    /// ahead of the others.
    fn hand_over(&mut self, repository: &RepositoryName, heads: Vec<PrHead>) {
        let mut known = self.take_heads(repository);
        known.prs.hand_over(heads);
        self.heads.push_front(known);
    }

    /// Puts the head of a PR the user opened in a fetch of its own, whatever the worker noted of it, and its
    /// Repository ahead of the others.
    fn put_first(&mut self, repository: &RepositoryName, head: PrHead) {
        let mut known = self.take_heads(repository);
        known.prs.fetch_first(head);
        self.heads.push_front(known);
    }

    /// Asks for each head of `heads` whose PR the user opened before the session knew its head, unless the
    /// mirror fetched that head since launch, and puts the Repository ahead of the others if it asked.
    fn saved(&mut self, repository: &RepositoryName, heads: Vec<PrHead>) {
        let known = (self.heads.iter_mut()).find(|known| known.repository.is_same(repository));
        let Some(known) = known else {
            return;
        };
        if known.prs.saved(heads) {
            self.wake();
            let known = self.take_heads(repository);
            self.heads.push_front(known);
        }
    }

    /// Starts the next git, unless one is under way, in the order `mirror_queue` picks as of `now`. A job
    /// can end before it starts a git, as when nothing it took needs one, and the next job is picked then.
    /// Says whether it stopped picking with jobs perhaps left, after a read of patch-ids.
    fn start_next(&mut self, now: Instant) -> bool {
        // Before the checks, so a wait that is over leaves `next_due` even while nothing can start.
        self.retry_failed(now);
        if !self.git_checked {
            self.check_git();
        }
        // A Check again forgot a short disk. The next clone says whether it still is, and with none in line
        // nothing is short.
        if self.problem.is_none() && self.waiting.is_empty() {
            self.set_problem(None);
        }
        while self.job.is_none() {
            let Some(next) = mirror_queue::next(&self.queue(now)) else {
                return false;
            };
            match next {
                Next::MoveLookup(repository) => self.start_lookup_move(&repository),
                Next::FetchDemanded(repository) => {
                    self.start_batch(&repository, |prs| prs.demanded(now).collect());
                }
                Next::FetchWanted(repository) => {
                    self.start_batch(&repository, |prs| prs.wanted(now).collect());
                }
                Next::KeepReviewed(repository) => self.start_keeping_reviewed(&repository),
                // One read at a time, so a message waits for one read at most. The loop picks again at
                // once, after the messages that came meanwhile.
                Next::ReadPatchIds(repository) => {
                    self.read_waiting_patch(&repository);
                    return true;
                }
                // `queue` puts the first in line up for its clone.
                Next::Clone(_) => {
                    if let Some(repository) = self.waiting.pop_front() {
                        self.try_clone(repository);
                    }
                }
                Next::Upkeep(repository) => self.start_upkeep(&repository),
            }
        }
        false
    }

    /// The worker's queue as of `now`, for `mirror_queue` to pick from. Only a Repository with a job waiting
    /// has its mirror looked for.
    fn queue(&self, now: Instant) -> Queue {
        let folders = self.folders.as_ref().ok();
        let there = |repository: &RepositoryName| {
            folders.is_some_and(|folders| folders.mirror(repository).exists())
        };
        let repositories = (self.heads.iter())
            .filter_map(|known| {
                let waits = [
                    (Waiting::LookupMove, known.lookup.waits()),
                    (Waiting::DemandedHeads, known.prs.demands(now)),
                    (Waiting::Heads, known.prs.wants(now)),
                    (Waiting::Reviewed, known.reviews.waits()),
                    (Waiting::PatchIds, known.patches.waits()),
                ];
                let waiting: BTreeSet<Waiting> = (waits.into_iter())
                    .filter_map(|(kind, waits)| waits.then_some(kind))
                    .collect();
                let ready = !waiting.is_empty() && there(&known.repository);
                ready.then(|| Queued {
                    repository: known.repository.clone(),
                    waiting,
                })
            })
            .collect();
        Queue {
            problem: self.problem.as_ref().map(MirrorProblem::holds_back),
            pace: self.pace,
            reads_patch_ids: self.patched.is_some() && self.old_for_check.is_none(),
            repositories,
            clone: self.waiting.front().cloned(),
            upkeep: self.upkeep_waiting(),
        }
    }

    /// Starts a clone of `repository` unless it has its mirror, or can't have one yet. Short of disk, it
    /// puts `repository` back first in line, and the clones wait.
    fn try_clone(&mut self, repository: RepositoryName) {
        let folders = match &self.folders {
            Ok(folders) => folders,
            Err(problem) => {
                log::warn!(
                    "Tsugiki can't clone {repository}'s mirror: {}",
                    problem.message
                );
                return;
            }
        };
        let folder = folders.repository(&repository);
        let mirror = folders.mirror(&repository);
        if mirror.exists() {
            return;
        }
        let partial = folder.join(PARTIAL);
        if partial.join(CLONED).exists() {
            self.settle(repository, &partial, &mirror);
            return;
        }
        let Some(source) = self.sources.get(&repository).cloned() else {
            self.asking.push(repository);
            self.tell_asking();
            return;
        };
        // What a clone cut off by quitting left goes first, or the disk would count it as taken.
        if partial.exists()
            && let Err(cause) = fs::remove_dir_all(&partial)
        {
            log::warn!(
                "Tsugiki can't delete the clone of {repository} that a quit cut off: {cause}"
            );
            self.fail_clone(repository, cause.to_string());
            return;
        }
        let root = folders.data.clone();
        match self.room_for(&repository, &root, &mirror) {
            Ok(None) => self.set_problem(None),
            Ok(Some(short)) => {
                self.waiting.push_front(repository);
                self.set_problem(Some(short));
                return;
            }
            Err(Unmeasured::PointsOut { back, words }) => {
                log::warn!(
                    "Tsugiki holds back the clone of {repository} until GitHub's points come back: {words}"
                );
                self.wait_clone(repository, words, back);
                return;
            }
            Err(Unmeasured::Other(cause)) => {
                log::warn!("Tsugiki can't say if {repository}'s mirror has room: {cause}");
                self.fail_clone(repository, cause);
                return;
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
                self.fail_clone(repository, cause.to_string());
            }
        }
    }

    /// Runs `git --version`, and notes git as missing or too old if it is, for the mirrors or for the Stale
    /// review check.
    fn check_git(&mut self) {
        self.git_checked = true;
        self.old_for_check = None;
        let problem = match self.git.read(&[OsStr::new("--version")]) {
            Ok(said) => match git_version(&said) {
                Some(version) if version >= OLDEST_GIT => {
                    self.old_for_check = (version < CHECK_GIT).then(|| said.trim().to_owned());
                    None
                }
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
        if problem.is_some() || self.told.as_ref().is_some_and(MirrorProblem::leaves_no_git) {
            self.set_problem(problem);
        }
    }

    /// Forgets what the worker found of git and the disk, and puts the clones that failed back in line.
    fn check_again(&mut self) {
        self.git_checked = false;
        self.problem = None;
        let failed = mem::take(&mut self.failed);
        self.waiting
            .extend(failed.into_iter().map(|failed| failed.repository));
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
    ) -> Result<Option<MirrorProblem>, Unmeasured> {
        let size = (self.remotes.size_of.size_of(repository)).map_err(Unmeasured::of)?;
        let needed = size.saturating_add(ROOM_TO_SPARE);
        let mut free = self.free_space(root).map_err(Unmeasured::Other)?;
        if free < needed && has_broken(mirror) {
            delete_broken(repository, mirror);
            // A broken mirror that couldn't be deleted gave back no room.
            let gone = !has_broken(mirror);
            let after = gone.then(|| self.free_space(root)).transpose();
            free = after.map_err(Unmeasured::Other)?.unwrap_or(free);
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

    /// Tries a clone of `repository`, which failed for the reason in `words`, again after a wait, which
    /// doubles each time it fails in a row.
    fn fail_clone(&mut self, repository: RepositoryName, words: String) {
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
        let retry_at = Instant::now() + clone_wait(self.remotes.retry, tries);
        self.wait_clone(repository, words, retry_at);
    }

    /// Puts the clone of `repository`, which failed for the reason in `words`, back in line at `retry_at`.
    fn wait_clone(&mut self, repository: RepositoryName, words: String, retry_at: Instant) {
        self.failed.push(FailedClone {
            repository,
            retry_at,
            words,
        });
    }

    /// Puts each clone that failed back in line once its wait is over.
    fn retry_failed(&mut self, now: Instant) {
        let (due, later) = mem::take(&mut self.failed)
            .into_iter()
            .partition(|failed| failed.retry_at <= now);
        self.failed = later;
        let due: Vec<FailedClone> = due;
        self.waiting
            .extend(due.into_iter().map(|failed| failed.repository));
    }

    /// The time the first head that is behind is due to be fetched once more, the first clone that failed
    /// is due to go back in line, the first mirror's upkeep falls due, or `start_next` said to pick again.
    /// A head whose moment was over at the turn's pick, `now`, waits for a pick after the next git or
    /// message.
    fn next_due(&self, pick_again: Option<Instant>, now: Instant) -> Option<Instant> {
        // Only an idle worker picks a head, and a git's end wakes it, so only it wakes for one.
        let idle = self.job.is_none().then_some(&self.heads);
        let heads = (idle.into_iter().flatten()).filter_map(|known| known.prs.next_due(now));
        let clones = self.failed.iter().map(|failed| failed.retry_at);
        // Only an idle worker with a usable git starts an upkeep, so only it wakes for one.
        let git_usable = !self
            .problem
            .as_ref()
            .is_some_and(MirrorProblem::leaves_no_git);
        let upkeep = (self.job.is_none() && git_usable).then(|| self.upkeep_due());
        heads
            .chain(clones)
            .chain(upkeep.flatten())
            .chain(pick_again)
            .min()
    }

    /// Tells each test that asked that the worker is idle, unless something is `due` at once: a read of
    /// patch-ids waits for the next turn of the loop. Something due a moment later doesn't count, so a test
    /// that says nothing started gives its retries and upkeep longer than it runs.
    #[cfg(test)]
    fn tell_idle(&mut self, due: Option<Instant>) {
        if due.is_none_or(|at| at > Instant::now()) {
            for reply in self.awaiting_idle.drain(..) {
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "a test that has stopped waiting doesn't hear it"
                )]
                let _ = reply.send(());
            }
        }
    }

    /// Starts a fetch of the heads `pick` picks from what the worker knows of `repository`'s heads: those
    /// the user asked for, in a fetch of their own, or those the rules want. The heads keep what they say
    /// until the fetch's outcome.
    fn start_batch(&mut self, repository: &RepositoryName, pick: impl FnOnce(&Prs) -> Vec<PrHead>) {
        let found = (self.heads.iter()).find(|known| known.repository.is_same(repository));
        if let (Ok(folders), Some(known)) = (&self.folders, found) {
            let batch = pick(&known.prs);
            let mirror = folders.mirror(&known.repository);
            let repository = known.repository.clone();
            self.start_fetch(repository, batch, mirror);
        }
    }

    /// Fetches each head of `batch` and its Target branch in one go. A glob in each refspec lets a head or
    /// a branch GitHub no longer has match nothing, where a plain one would fail the whole fetch (#157),
    /// and `--prune` deletes its old ref, so the refs read back say which heads GitHub has. A branch's glob
    /// also brings the branches whose names start with its name, which costs little. A connection that
    /// stalls for a minute fails the fetch, which would otherwise hold up every git after it.
    fn start_fetch(&mut self, repository: RepositoryName, batch: Vec<PrHead>, mirror: PathBuf) {
        let heads = (batch.iter()).map(|PrHead { number, .. }| {
            format!("+refs/pull/{number}/head*:refs/tsugiki/pull/{number}/head*\n")
        });
        let targets: BTreeSet<&str> = (batch.iter())
            .map(|head| head.target_branch.as_str())
            .collect();
        let branches =
            (targets.iter()).map(|branch| format!("+refs/heads/{branch}*:refs/heads/{branch}*\n"));
        let refspecs: String = heads.chain(branches).collect();
        let args = fetch_args(&mirror, &["--prune"]);
        match self.git.start(&args, refspecs.as_bytes()) {
            Ok(started) => {
                self.listen(started.output);
                self.job = Some(Job::Fetch(Fetch {
                    repository,
                    mirror,
                    batch,
                    started: Instant::now(),
                    git: started.run,
                    news: false,
                }));
            }
            Err(cause) => {
                log::warn!("Tsugiki can't start fetching PR heads of {repository}: {cause}");
                let numbers: Vec<u32> = batch.iter().map(|head| head.number).collect();
                self.try_again_later(&repository);
                self.pace = Pace::Stalled;
                let why = Unfetched::Git {
                    words: cause.to_string(),
                };
                (self.announce.failed)(&repository, &numbers, &why);
            }
        }
    }

    /// Puts a Repository's heads back after a fetch that failed. The fetch changed nothing of them, so each
    /// waits as it did, with any hand-over that came meanwhile.
    fn try_again_later(&mut self, repository: &RepositoryName) {
        let known = self.take_heads(repository);
        self.requeue(known);
    }

    /// Puts a Repository's heads back after its fetch: behind the other Repositories', so each takes its
    /// turn, unless a head the user asked for waits.
    fn requeue(&mut self, known: Heads) {
        if known.prs.asks_wait() {
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
    /// ref for it. `Prs::note_fetched` says what follows.
    fn finish_fetch(&mut self, fetch: Fetch, last_words: &str) {
        let Fetch {
            repository,
            mirror,
            batch,
            started,
            mut git,
            news,
        } = fetch;
        if !git.succeeded() {
            let numbers: Vec<u32> = batch.iter().map(|head| head.number).collect();
            self.try_again_later(&repository);
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
            Unfetched::Git {
                words: cause.to_string(),
            }
        });
        if let Ok(landed) = &landed {
            for PrHead { number, commit, .. } in &batch {
                if let Some(at) = landed.get(number).filter(|at| *at != commit) {
                    let (had, asked) = (at.as_str(), commit.as_str());
                    log::info!(
                        "GitHub had PR #{number} of {repository} at {had} rather than {asked}"
                    );
                }
            }
        }
        let mut known = self.take_heads(&repository);
        let retry = Retry {
            started,
            at: Instant::now() + self.remotes.patience,
        };
        let noted = known.prs.note_fetched(&batch, landed.as_ref(), retry);
        if !noted.arrived.is_empty() {
            known.patches.read_missing_again();
        }
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

    /// Tells the requests each Repository whose clone waits for an answer, and the window the first, if that
    /// changed.
    fn tell_asking(&mut self) {
        self.asked.set(&self.asking);
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

    /// Tells the window the state of each mirror the worker has heard of, if one changed.
    fn tell_states(&mut self) {
        let states: Vec<RepositoryMirror> = (self.heard.iter())
            .map(|repository| RepositoryMirror {
                repository: repository.clone(),
                state: self.state_of(repository),
            })
            .collect();
        if states != self.told_states {
            (self.announce.states)(&states);
            self.told_states = states;
        }
    }

    /// The state of `repository`'s mirror, which the worker has heard of.
    fn state_of(&self, repository: &RepositoryName) -> MirrorState {
        let cloning = match &self.job {
            Some(Job::Clone(clone)) => Some(&clone.cloning),
            Some(Job::Fetch(_) | Job::Reviewed(_) | Job::Upkeep(_) | Job::Lookup(_)) | None => None,
        };
        if let Some(cloning) = cloning.filter(|cloning| cloning.repository.is_same(repository)) {
            return MirrorState::Cloning {
                progress: cloning.progress,
            };
        }
        if self.asking.iter().any(|one| one.is_same(repository)) {
            return MirrorState::Asking;
        }
        if let Some(failed) =
            (self.failed.iter()).find(|failed| failed.repository.is_same(repository))
        {
            return MirrorState::Failed {
                words: failed.words.clone(),
            };
        }
        if !self.waiting.iter().any(|one| one.is_same(repository)) {
            return MirrorState::Ready;
        }
        match &self.problem {
            // A disk short for one clone holds back every clone until a Check again.
            Some(problem) => MirrorState::Problem {
                problem: problem.clone(),
            },
            None => match cloning {
                Some(cloning) => MirrorState::Waiting {
                    behind: Some(cloning.repository.clone()),
                },
                None if self.pace == Pace::Held => MirrorState::Offline,
                None => MirrorState::Waiting { behind: None },
            },
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
        #[expect(
            unused_results,
            reason = "it ends with git's error output, and its last message tells the worker so"
        )]
        thread::spawn(move || read_progress_until_done(output, &messages));
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
        if clone.git.succeeded() {
            self.settle(repository, &clone.partial, &clone.mirror);
        } else {
            log::warn!("Tsugiki couldn't clone {repository}'s mirror: {last_words}");
            #[expect(
                clippy::let_underscore_must_use,
                reason = "what is left goes before the next clone of this Repository, which says if it can't"
            )]
            let _ = fs::remove_dir_all(&clone.partial);
            self.fail_clone(repository, last_words.to_owned());
        }
        (self.announce.cloning)(None);
    }

    /// Moves the clone git finished in `partial` to `mirror`. If Windows refuses, as another program has a
    /// file in it open, the clone stays marked as finished, and its retry moves it.
    fn settle(&mut self, repository: RepositoryName, partial: &Path, mirror: &Path) {
        match fs::rename(partial, mirror) {
            Ok(()) => {
                log::info!("Tsugiki cloned {repository}'s mirror");
                // Only a clone that couldn't be moved at once has the mark. git doesn't read it, so one left
                // behind does no harm.
                if let Err(cause) = fs::remove_file(mirror.join(CLONED))
                    && cause.kind() != io::ErrorKind::NotFound
                {
                    log::warn!("Tsugiki can't delete the mark in {repository}'s mirror: {cause}");
                }
                self.clone_tries
                    .retain(|(one, _)| !one.is_same(&repository));
                self.cloned_afresh(&repository, mirror);
            }
            Err(cause) => {
                log::warn!(
                    "Tsugiki can't move {repository}'s finished clone to its mirror: {cause}"
                );
                mark_cloned(&repository, partial);
                self.fail_clone(repository, cause.to_string());
            }
        }
    }
}

/// Marks the clone of `repository` in `partial` as one git finished, so it is kept. Without the mark, the
/// next clone deletes it and starts again.
fn mark_cloned(repository: &RepositoryName, partial: &Path) {
    if let Err(cause) = fs::write(partial.join(CLONED), "") {
        log::warn!("Tsugiki can't mark {repository}'s clone as finished: {cause}");
    }
}

/// The time a stopped git's helpers get to end, after their kill. Ending takes a moment, so only a helper
/// that left git's job object or process group, and didn't get the kill, outlasts this.
const HELPERS_PATIENCE: Duration = Duration::from_secs(5);

/// Waits for the error output of the git stopped last to close, up to `HELPERS_PATIENCE`. Each helper holds
/// it, so it closes once the last of them has ended. `Run::stop_finished` waits for git, and on Windows the
/// job object's wait returns before the helpers it killed have ended (#208).
fn wait_for_helpers(incoming: &Receiver<Message>) {
    let deadline = Instant::now() + HELPERS_PATIENCE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match incoming.recv_timeout(left) {
            Ok(Message::Ended { .. }) | Err(_) => break,
            // Stopping drops what else comes.
            Ok(_) => {}
        }
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

/// The arguments of a `git fetch` into `mirror` of the refspecs on its input, with `extra` options. A mirror
/// cloned before Tsugiki kept it has no `maintenance.auto=false` of its own, and a connection that stalls
/// for a minute fails the fetch.
fn fetch_args<'a>(mirror: &'a Path, extra: &[&'a str]) -> Vec<&'a OsStr> {
    let config = [
        "-c",
        "maintenance.auto=false",
        "-c",
        "http.lowSpeedLimit=1",
        "-c",
        "http.lowSpeedTime=60",
        "-C",
    ];
    let options = ["--stdin", "--no-tags", "--no-write-fetch-head"];
    (config.into_iter().map(OsStr::new))
        .chain([mirror.as_os_str(), OsStr::new("fetch")])
        .chain(
            options
                .into_iter()
                .chain(extra.iter().copied())
                .map(OsStr::new),
        )
        .chain([OsStr::new("origin")])
        .collect()
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
            Some(progress) => tell_worker(messages, Message::Progress(progress)),
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
    tell_worker(messages, Message::Ended { last_words });
}

/// Sends the worker `message` about the git it waits for.
fn tell_worker(messages: &Sender<Message>, message: Message) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "this fails only once the worker has ended, and then nothing waits for git"
    )]
    let _ = messages.send(message);
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
