//! The one way to ask for data from GitHub or the Mirrors (ADR 0022). Two actors ask. The user asks through
//! the window, with demands: urgent, and the newer wins where two conflict. The rules in `work` state wants:
//! the whole set for a Repository each time, which replaces the last.
//!
//! [`Fetching`] is a handle with no thread of its own, which the app and the session each hold a clone of.
//! Two queues stay behind it, each with one owner: the session runs GitHub's, one request at a time (ADR
//! 0015), and the Mirror worker runs git's. Each ranks the user's demands first, then the rules' wants, then
//! its own background work, such as the sync's tiers or a Mirror's upkeep.

use std::path::Path;
use std::sync::mpsc::{self, Sender};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::patch_ids::PatchesOf;
use crate::facts::pull_requests::{HeadInMirror, PrHead, ReviewedCommit};
use crate::mirror::{CloneRefusal, LookupAt, MirrorRequests};
use crate::mirror_source::MirrorSource;
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositoryCheck, RepositorySettings, SavedBy};
use crate::work::SeenMark;

pub(crate) const NOT_CONNECTED: &str =
    "Tsugiki isn't connected to GitHub. The status bar says why.";

/// What the rules want of a Repository's Mirror, worked out each time a sync step saves the Repository's
/// facts. Each part is the whole set, and replaces the last one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorWant {
    /// The heads of the PRs in Reach, to fetch.
    pub heads: Vec<PrHead>,
    /// The commits the user's Reviews covered, to keep, or nothing when the store couldn't read them: the
    /// Mirror worker then goes on with the last want's.
    pub reviewed: Option<Vec<ReviewedCommit>>,
    /// The commits whose patch-ids the Stale review check needs and the store lacks, to read.
    pub patches: Vec<PatchesOf>,
}

/// An Issue or a PR of a Repository, in a message to the session's thread.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) repository: RepositoryName,
    pub(crate) number: u32,
}

/// A mark the user made on an item: seen or unseen.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Marked {
    Seen {
        repository: RepositoryName,
        mark: SeenMark,
    },
    /// The flag starts at `at`.
    Unseen { item: Item, at: OffsetDateTime },
}

/// An Issue or a PR on screen, whose conversation the session reads from GitHub.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fetched {
    Issue(Item),
    PullRequest(Item),
}

impl Fetched {
    pub(crate) fn item(&self) -> &Item {
        let (Self::Issue(item) | Self::PullRequest(item)) = self;
        item
    }
}

/// What the session's thread is asked, which alone holds the token, the client and the store's writer. It
/// takes each between two sync steps, so the user's demands go ahead of the sync's tiers.
pub(crate) enum SessionMessage {
    Stop,
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
    /// The user opened this Repository.
    Sync {
        repository: RepositorySettings,
    },
    /// The user asked to refresh the open Repository now.
    Refresh,
    /// The user changed what these settings say, and nothing on GitHub changed with them.
    SettingsChanged(RepositorySettings, SavedBy),
    Mark(Marked),
    Fetch(Fetched),
    /// The per-file patch-ids the Mirror worker read, or the commits it can't read, for the store to keep.
    Patched(RepositoryName, PatchReads),
}

/// Keeps the per-file patch-ids read of commits in a Repository, and the commits that can't be read, as
/// `Fetching::patch_ids_keeper` gives it.
pub type KeepPatchIds = Box<dyn FnMut(&RepositoryName, PatchReads) + Send>;

/// Asks GitHub and the Mirrors for data, from any thread. Each call returns at once, but for `check` and
/// `check_clone`. A check waits for what the session is doing first, which at the start can be `gh` or a
/// slow request, so don't call it from the thread that draws the window.
#[derive(Clone)]
pub struct Fetching {
    session: Sender<SessionMessage>,
    mirrors: MirrorRequests,
}

impl Fetching {
    /// A handle on the session's queue, which `session` sends to, and on the Mirror worker's.
    pub(crate) fn new(session: Sender<SessionMessage>, mirrors: MirrorRequests) -> Self {
        Self { session, mirrors }
    }

    /// Tells the session to finish.
    pub(crate) fn stop_session(&self) {
        self.to_session(SessionMessage::Stop);
    }

    /// The user opened `repository`: the session loads the user's work and its Filters. A session with no
    /// token or no database has already shown why on the status bar, so this reports no failure.
    pub fn sync(&self, repository: RepositorySettings) {
        self.to_session(SessionMessage::Sync { repository });
    }

    /// The user asked to refresh the open Repository now. A sync already under way covers this.
    pub fn refresh(&self) {
        self.to_session(SessionMessage::Refresh);
    }

    /// The settings owner saved `repository`'s settings otherwise, as `by` did. The session's syncs read them
    /// from now on, if it has opened the Repository since launch, and the Mirror worker and the read ahead
    /// follow the Reach they make. Which Repository is open stays, and no refresh starts.
    pub fn settings_changed(&self, repository: RepositorySettings, by: SavedBy) {
        self.to_session(SessionMessage::SettingsChanged(repository, by));
    }

    /// An Issue on screen: the session reads what it says now. Only Mark Seen moves Last seen, so opening
    /// the Issue or reading it again leaves it alone.
    pub fn fetch_issue(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.to_session(SessionMessage::Fetch(Fetched::Issue(item)));
    }

    /// A PR on screen: the session reads its Conversation now. Like `fetch_issue`, this leaves Last seen
    /// alone.
    pub fn fetch_pull_request(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.to_session(SessionMessage::Fetch(Fetched::PullRequest(item)));
    }

    /// Mark Seen on what `mark` names, as the window drew it. A session that can't ask GitHub still records
    /// it.
    pub fn mark_seen(&self, repository: RepositoryName, mark: SeenMark) {
        self.to_session(SessionMessage::Mark(Marked::Seen { repository, mark }));
    }

    /// Mark Unseen on item `number`, from now.
    pub fn mark_unseen(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        let at = OffsetDateTime::now_utc();
        self.to_session(SessionMessage::Mark(Marked::Unseen { item, at }));
    }

    /// Asks GitHub what adding `name` would load. A session with no token or no database answers with an
    /// error, and has already shown why on the status bar.
    pub fn check(&self, name: RepositoryName) -> Result<RepositoryCheck, Error> {
        let (reply, answer) = mpsc::channel();
        self.session
            .send(SessionMessage::Check { name, reply })
            .ok()
            .and_then(|()| answer.recv().ok())
            .unwrap_or_else(|| Err(Error::new(ErrorKind::Offline, NOT_CONNECTED)))
    }

    /// What keeps the per-file patch-ids the Mirror worker read of commits in a Repository, and the commits
    /// it can't read. Each call returns at once, and the window then hears that the PRs whose reviewed
    /// commit or head they are changed. It needs only the database, so a session that can't reach GitHub
    /// keeps them too.
    pub(crate) fn patch_ids_keeper(&self) -> KeepPatchIds {
        let session = self.session.clone();
        Box::new(move |repository, patches| {
            let patched = SessionMessage::Patched(repository.clone(), patches);
            drop(session.send(patched));
        })
    }

    /// Has the Mirror worker hand the patch-ids it reads to the session from now on.
    pub(crate) fn hand_patch_ids_to_session(&self) {
        self.mirrors.hand_patches_to(self.patch_ids_keeper());
    }

    /// Clones `repository`'s mirror after the ones already waiting, unless it has one. With no `source`, the
    /// clone waits until the user says where it gets its objects, or a request with one comes.
    pub fn clone_mirror(&self, repository: RepositoryName, source: Option<MirrorSource>) {
        self.mirrors.clone_mirror(repository, source);
    }

    /// The user wants `repository`'s mirror: its clone goes before the ones waiting, after the one under way.
    pub fn clone_mirror_next(&self, repository: RepositoryName, source: Option<MirrorSource>) {
        self.mirrors.clone_mirror_next(repository, source);
    }

    /// Says whether the clone in `folder` can lend its objects to `repository`'s mirror. It runs git commands
    /// that only read the clone, and waits for them.
    pub fn check_clone(
        &self,
        folder: &Path,
        repository: &RepositoryName,
    ) -> Result<(), CloneRefusal> {
        self.mirrors.check_clone(folder, repository)
    }

    /// Whether `repository`'s clone waits for the user to say where its mirror gets its objects.
    #[must_use]
    pub fn mirror_asks(&self, repository: &RepositoryName) -> bool {
        self.mirrors.asks(repository)
    }

    /// The Mirror worker looks at git and the disk again, and tries each clone that waits for them or failed.
    pub fn check_mirrors_again(&self) {
        self.mirrors.check_again();
    }

    /// The user opened PR `number`, whose head the Mirror holds as `head` says. Its ref stays until the next
    /// launch. A missing head goes ahead of the heads the rules want, and an unknown one does too once a
    /// sync step saves it.
    pub fn pull_request_opened(&self, repository: RepositoryName, number: u32, head: HeadInMirror) {
        match head {
            HeadInMirror::Unknown => self.mirrors.await_head(repository, number),
            HeadInMirror::Missing(head) => self.mirrors.fetch_first(repository, head),
            HeadInMirror::Held => self.mirrors.opened(repository, number),
        }
    }

    /// The Files tab shows the diff between the ends in `at`, so `repository`'s **Lookup checkout** moves
    /// there, ahead of the Mirror worker's other work. A newer move replaces one still waiting.
    pub fn look_up(&self, repository: RepositoryName, at: LookupAt) {
        self.mirrors.look_up(repository, at);
    }

    /// What the rules want of `repository`'s Mirror now, which replaces what they wanted before.
    pub(crate) fn want(&self, repository: &RepositoryName, want: MirrorWant) {
        let MirrorWant {
            heads,
            reviewed,
            patches,
        } = want;
        self.mirrors.fetch_heads(repository.clone(), heads);
        if let Some(reviewed) = reviewed {
            self.mirrors.keep_reviewed(repository.clone(), reviewed);
        }
        self.mirrors.read_patch_ids(repository.clone(), patches);
    }

    /// A sync step saved `heads` of `repository`'s PRs, so the head of a PR the user opened before its
    /// head was known goes first.
    pub(crate) fn heads_saved(&self, repository: &RepositoryName, heads: Vec<PrHead>) {
        self.mirrors.heads_saved(repository.clone(), heads);
    }

    fn to_session(&self, message: SessionMessage) {
        // The session has stopped, or has shown on the status bar why it can't ask GitHub.
        drop(self.session.send(message));
    }
}

#[cfg(test)]
mod tests;
