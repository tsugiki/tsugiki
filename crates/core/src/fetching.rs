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
use std::sync::{Arc, Mutex, PoisonError};

use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::activity::{ItemKey, PutAway};
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
    /// The PRs out of Reach whose refs upkeep keeps, those of the Archived trees (#820).
    pub kept: Vec<u32>,
    /// The commits the user's Reviews covered, to keep, or nothing when the store couldn't read them: the
    /// Mirror worker then goes on with the last want's.
    pub reviewed: Option<Vec<ReviewedCommit>>,
    /// The commits whose patch-ids the Stale review check needs and the store lacks, to read.
    pub patches: Vec<PatchesOf>,
}

/// An Issue or a PR of a Repository, in a message to the session's thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    pub(crate) repository: RepositoryName,
    pub(crate) number: u32,
}

impl Item {
    /// Whether `other` names this item, whatever the case of its Repository's name.
    pub(crate) fn is(&self, other: &Self) -> bool {
        self.repository.is_same(&other.repository) && self.number == other.number
    }
}

/// A mark the user made on an item: seen or unseen, archived or not, muted or not.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Marked {
    Seen {
        repository: RepositoryName,
        mark: SeenMark,
    },
    /// The flag on Issue or PR `number` starts at `at`, and `archived`, the item the window draws Archived
    /// that holds it, if any, is unarchived.
    Unseen {
        repository: RepositoryName,
        number: u32,
        archived: Option<ItemKey>,
        at: OffsetDateTime,
    },
    /// Archive on the item `key` names, with the Mark Seen the window drew for it, at `at`, putting the item
    /// away as `put_away` says.
    Archived {
        repository: RepositoryName,
        key: ItemKey,
        mark: SeenMark,
        put_away: PutAway,
        at: OffsetDateTime,
    },
    Unarchived {
        repository: RepositoryName,
        key: ItemKey,
    },
    /// Mute on the item `key` names, with the Mark Seen the window drew for it, at `at` (#492).
    Muted {
        repository: RepositoryName,
        key: ItemKey,
        mark: SeenMark,
        at: OffsetDateTime,
    },
    Unmuted {
        repository: RepositoryName,
        key: ItemKey,
    },
}

impl Marked {
    /// Archive on the item `key` names now, with the Mark Seen the window drew for it in `mark`, putting the
    /// item away as `put_away` says.
    fn archived(
        repository: RepositoryName,
        key: ItemKey,
        mark: SeenMark,
        put_away: PutAway,
    ) -> Self {
        Self::Archived {
            repository,
            key,
            mark,
            put_away,
            at: OffsetDateTime::now_utc(),
        }
    }
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

    /// Whether `other` is the same read: of the same kind, of the same item.
    pub(crate) fn is(&self, other: &Self) -> bool {
        let same_kind = matches!(
            (self, other),
            (Self::Issue(_), Self::Issue(_)) | (Self::PullRequest(_), Self::PullRequest(_))
        );
        same_kind && self.item().is(other.item())
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
    /// A read of what an item says. A demand that comes while the same read waits or runs shares its outcome.
    Fetch(Fetched),
    /// The per-file patch-ids the Mirror worker read, or the commits it can't read, for the store to keep.
    Patched(RepositoryName, PatchReads),
}

/// A job the core queued, which brings the data an answer waits for. Only `Fetching` makes one, as it queues
/// the job, so an answer that says its data is on the way can't be built while nothing brings it (ADR 0023).
/// It has no `Clone`, so each answer holds a job of its own.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub struct Queued(());

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
    headless: Headless,
}

/// The PRs whose Conversation, as the session last read it since launch, named no head, and whose
/// Conversation no demand has asked to read since. The session adds a PR when its read names no head, and a
/// demand for the Conversation takes the PR off, so the read it asks for decides again. Each method takes
/// the lock for one action, and no guard leaves the type. Nothing in it can panic while it holds the lock, so
/// it reads through a poisoned one.
#[derive(Clone, Default)]
pub(crate) struct Headless(Arc<Mutex<Vec<Item>>>);

impl Headless {
    /// The session's read of PR `item`'s Conversation named no head. A demand came before each read and took
    /// the PR off, and `forget` takes off every copy, so a PR is never held twice for long.
    pub(crate) fn named_none(&self, item: &Item) {
        let mut items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        items.push(item.clone());
    }

    /// PR `item` has a head, or the read of its Conversation that a demand asked for decides it.
    pub(crate) fn forget(&self, item: &Item) {
        let mut items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        items.retain(|headless| !headless.is(item));
    }

    fn holds(&self, item: &Item) -> bool {
        let items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        items.iter().any(|headless| headless.is(item))
    }
}

impl Fetching {
    /// A handle on the session's queue, which `session` sends to, and on the Mirror worker's, with the PRs
    /// whose Conversation the session read named no head.
    pub(crate) fn new(
        session: Sender<SessionMessage>,
        mirrors: MirrorRequests,
        headless: Headless,
    ) -> Self {
        Self {
            session,
            mirrors,
            headless,
        }
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
    /// alone. Whether the PR has a head waits for this read, whatever the last one named.
    pub fn fetch_pull_request(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.headless.forget(&item);
        self.to_session(SessionMessage::Fetch(Fetched::PullRequest(item)));
    }

    /// Whether the session's last read of PR `number`'s Conversation since launch named no head, and no
    /// demand has asked for another read since. A Conversation saved by an older Tsugiki, which kept no
    /// head, hasn't been read yet.
    pub fn named_no_head(&self, repository: &RepositoryName, number: u32) -> bool {
        self.headless.holds(&Item {
            repository: repository.clone(),
            number,
        })
    }

    /// PR `number`'s Conversation named no head, or the facts hold no such PR, so nothing will answer the
    /// Mirror worker's await of its head.
    pub(crate) fn no_head(&self, repository: RepositoryName, number: u32) {
        self.mirrors.no_head(repository, number);
    }

    /// Mark Seen on what `mark` names, as the window drew it. A session that can't ask GitHub still records
    /// it.
    pub fn mark_seen(&self, repository: RepositoryName, mark: SeenMark) {
        self.to_session(SessionMessage::Mark(Marked::Seen { repository, mark }));
    }

    /// Mark Unseen on Issue or PR `number`, from now. It unarchives `archived`, the item that holds it, when
    /// the window draws that Archived (#491).
    pub fn mark_unseen(&self, repository: RepositoryName, number: u32, archived: Option<ItemKey>) {
        let at = OffsetDateTime::now_utc();
        self.to_session(SessionMessage::Mark(Marked::Unseen {
            repository,
            number,
            archived,
            at,
        }));
    }

    /// Archive on the item `key` names: Mark Seen with `mark`, as the window drew it, and the item put away
    /// now, until something new Needs attention there.
    pub fn archive(&self, repository: RepositoryName, key: ItemKey, mark: SeenMark) {
        let archived = Marked::archived(repository, key, mark, PutAway::Now);
        self.to_session(SessionMessage::Mark(archived));
    }

    /// Mark Seen with `mark` on the item `key` names, which the window draws Archived: Archive that keeps
    /// the item Archived in its place in the Archived view (#491).
    pub fn keep_archived(&self, repository: RepositoryName, key: ItemKey, mark: SeenMark) {
        let archived = Marked::archived(repository, key, mark, PutAway::AsBefore);
        self.to_session(SessionMessage::Mark(archived));
    }

    /// Unarchives the item `key` names.
    pub fn unarchive(&self, repository: RepositoryName, key: ItemKey) {
        self.to_session(SessionMessage::Mark(Marked::Unarchived { repository, key }));
    }

    /// Mute on the item `key` names: Mark Seen with `mark`, as the window drew it, and the item hidden until
    /// the user unmutes it (#492).
    pub fn mute(&self, repository: RepositoryName, key: ItemKey, mark: SeenMark) {
        let at = OffsetDateTime::now_utc();
        let muted = Marked::Muted {
            repository,
            key,
            mark,
            at,
        };
        self.to_session(SessionMessage::Mark(muted));
    }

    /// Unmutes the item `key` names.
    pub fn unmute(&self, repository: RepositoryName, key: ItemKey) {
        self.to_session(SessionMessage::Mark(Marked::Unmuted { repository, key }));
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

    /// A screen waits for `repository`'s Mirror: its clone goes on, or takes its turn after the ones waiting.
    /// A clone under way, waiting or failed stays as it is.
    pub fn clone_shown(&self, repository: RepositoryName) -> Queued {
        self.mirrors.clone_mirror(repository, None);
        Queued(())
    }

    /// A screen waits for the head of PR `number`, which Tsugiki doesn't know: the session reads the PR's
    /// Conversation, which names it, or this shares a read of it already waiting or under way. The Mirror
    /// worker keeps the PR's ref, and fetches the head first once the session saves it.
    pub fn find_head(&self, repository: RepositoryName, number: u32) -> Queued {
        self.mirrors.await_head(repository.clone(), number);
        self.fetch_pull_request(repository, number);
        Queued(())
    }

    /// A screen waits for `head`, of a PR the user opened, which `repository`'s Mirror lacks. It goes ahead
    /// of the heads the rules want, and the PR's ref stays until the next launch.
    pub fn fetch_head(&self, repository: RepositoryName, head: PrHead) -> Queued {
        self.mirrors.fetch_first(repository, head);
        Queued(())
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
            kept,
            reviewed,
            patches,
        } = want;
        // Before the heads, so no upkeep between the two prunes a ref the PR's move out of Reach keeps.
        self.mirrors.keep_refs(repository.clone(), kept);
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
pub(crate) mod testing;
#[cfg(test)]
mod tests;
