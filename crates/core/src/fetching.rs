//! The one way to ask for data from GitHub or the Mirrors (ADR 0022). Two actors ask. The user asks through
//! the window, with demands: urgent, and the newer wins where two conflict. The rules in `work` state wants:
//! the whole set for a Repository each time, which replaces the last.
//!
//! [`Fetching`] is a handle with no thread of its own, which the session and the core's calls each hold a
//! clone of. Only the core can name it: the app asks through `calls` (ADR 0023). Two queues stay behind it,
//! each with one owner: the session's GitHub thread runs GitHub's, one request at a time (ADR 0015), and the
//! Mirror worker runs git's. Each ranks the user's demands first, then the rules' wants, then its own
//! background work, such as the sync's tiers or a Mirror's upkeep. The session's own thread takes what this
//! handle sends it at once, and passes what needs GitHub on to the GitHub thread.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::error::{Error, ErrorKind};
use crate::facts::activity::{ItemKey, PutAway, ShowMuted};
use crate::facts::patch_ids::PatchesOf;
use crate::facts::pull_requests::{HeadInMirror, PrHead, ReviewedCommit};
use crate::login::Login;
use crate::mirror::{CloneRefusal, LookupAt, MirrorRequests};
use crate::mirror_source::MirrorSource;
use crate::patch_ids::PatchReads;
use crate::repository::RepositoryName;
use crate::repository_settings::{RepositoryCheck, SettingsSave};
use crate::status::{Change, ItemUnread};
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
    /// Mute author on `author` in `repository`, at `at` (#494). The session finds their open items, whose
    /// trees the mute marks seen, by the link rules under the Repository's settings as it takes the mark.
    AuthorMuted {
        repository: RepositoryName,
        author: Login,
        at: OffsetDateTime,
    },
    /// Unmute author on `author` in `repository`, under whose settings as the session takes the mark it finds
    /// the items that come back.
    AuthorUnmuted {
        repository: RepositoryName,
        author: Login,
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

    /// Whether the mark moves Reach: Mute and Unmute, on an item or an author (#918).
    pub(crate) fn moves_reach(&self) -> bool {
        match self {
            Self::Muted { .. }
            | Self::Unmuted { .. }
            | Self::AuthorMuted { .. }
            | Self::AuthorUnmuted { .. } => true,
            Self::Seen { .. }
            | Self::Unseen { .. }
            | Self::Archived { .. }
            | Self::Unarchived { .. } => false,
        }
    }

    /// The Repository whose item or author the mark is on.
    pub(crate) fn repository(&self) -> &RepositoryName {
        match self {
            Self::Seen { repository, .. }
            | Self::Unseen { repository, .. }
            | Self::Archived { repository, .. }
            | Self::Unarchived { repository, .. }
            | Self::Muted { repository, .. }
            | Self::Unmuted { repository, .. }
            | Self::AuthorMuted { repository, .. }
            | Self::AuthorUnmuted { repository, .. } => repository,
        }
    }
}

/// An Issue or a PR on screen, whose conversation the session reads from GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// What the user did with an Issue or a PR in the window, or with the window itself, which says what the
/// session reads from GitHub again each minute.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScreenMove {
    /// A view opened this item, which shows. Each view of an item sends one, as two can overlap while one
    /// takes over from the other.
    Opened(Fetched),
    /// Another view hides this item, which stays open.
    Hid(Item),
    /// This item shows again.
    Showed(Item),
    /// A view of this item went, and the item goes off screen once all its views have.
    Closed(Item),
    WindowHid,
    WindowShowed,
    /// The window loaded afresh and shows: the views of the page before went without closing.
    WindowLoaded,
}

/// What the session's thread is asked. It takes each at once: it saves a mark or the Mirror's patch-ids
/// itself, and passes a demand on GitHub to the GitHub thread, which alone holds the token and the client,
/// and takes it between two sync steps, so the user's demands go ahead of the sync's tiers.
pub(crate) enum SessionMessage {
    Stop,
    Check {
        name: RepositoryName,
        reply: Sender<Result<RepositoryCheck, Error>>,
    },
    /// The user opened this Repository, whose settings the session reads from the settings owner's view.
    Sync {
        repository: RepositoryName,
    },
    /// The user asked to refresh the open Repository now.
    Refresh,
    /// The user changed what a Repository's settings say, and nothing on GitHub changed with them.
    SettingsChanged(SettingsSave),
    Mark(Marked),
    /// A read of what an item says. A demand that comes while the same read waits or runs shares its outcome.
    Fetch(Fetched),
    /// What the user did with an item on screen, or with the window.
    Screen(ScreenMove),
    /// The per-file patch-ids the Mirror worker read, or the commits it can't read, for the store to keep.
    Patched(RepositoryName, PatchReads),
    /// The GitHub thread saved facts of a Repository that can change what the rules want of the Mirror, and
    /// of the items the change names. The session works the wants out and keeps the checks, as it does after
    /// a mark, so one thread writes what the rules make of the facts. Then it hands the worker those items'
    /// heads.
    Saved(Change),
    /// A fetch of an Issue or a PR the database lacks, which answers what it found (#981).
    FetchUnstored {
        item: Item,
        reply: Sender<ItemFetch>,
    },
    /// "Show muted" on the Inbox of `repository` turned to `show` (#825), which answers once the store has
    /// kept it, or why it couldn't. The window then asks for the Inbox again.
    ShowMuted {
        repository: RepositoryName,
        show: ShowMuted,
        reply: Sender<Result<(), Error>>,
    },
}

/// What a fetch of an Issue or a PR the database lacks found (#981). GitHub shares one number space between
/// Issues and PRs, so the number turns out to be either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemFetch {
    /// An Issue, which the database now holds.
    Issue,
    /// A PR, which the database now holds.
    PullRequest,
    /// Nothing the database now holds, for this reason.
    Unfetched { why: ItemUnfetched },
}

/// Why a fetch of an Issue or a PR the database lacks brought nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemUnfetched {
    /// GitHub has no Issue or PR with that number in the Repository.
    Missing,
    /// The token can't see the item, or its Repository.
    Hidden,
    /// GitHub can't be asked now, as when the computer is offline, Tsugiki has no token or the hour's points
    /// ran out, and the status bar says why.
    Offline,
    /// Anything else, such as an answer Tsugiki couldn't read or a database it couldn't write. The log says.
    Other,
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
/// `check_clone`. A check waits for what the GitHub thread is doing first, which at the start can be `gh` or
/// a slow request, so don't call it from the thread that draws the window.
#[derive(Clone)]
pub struct Fetching {
    session: Sender<SessionMessage>,
    mirrors: MirrorRequests,
    headless: Headless,
}

/// The reason the session's last read of a PR's Conversation since launch left the PR with no head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoHead {
    /// GitHub answered, and the Conversation named no head commit.
    Unnamed,
    /// The read failed, and no read until the next one the session makes would bring the head.
    Unread(ItemUnread),
}

/// The PRs whose Conversation, as the session last read it since launch, left them with no head, and whose
/// Conversation no demand has asked to read since. The session adds a PR when its read names no head or
/// fails, and a demand for the Conversation takes the PR off, so the read it asks for decides again. Each
/// method takes the lock for one action, and no guard leaves the type. Nothing in it can panic while it holds
/// the lock, so it reads through a poisoned one.
#[derive(Clone, Default)]
pub(crate) struct Headless(Arc<Mutex<Vec<(Item, NoHead)>>>);

impl Headless {
    /// The session's read of PR `item`'s Conversation left it with no head, as `why` says, in place of what
    /// the read before said.
    pub(crate) fn note(&self, item: &Item, why: NoHead) {
        let mut items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        items.retain(|(headless, _)| !headless.is(item));
        items.push((item.clone(), why));
    }

    /// PR `item` has a head, or the read of its Conversation that a demand asked for decides it.
    pub(crate) fn forget(&self, item: &Item) {
        let mut items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        items.retain(|(headless, _)| !headless.is(item));
    }

    fn why(&self, item: &Item) -> Option<NoHead> {
        let items = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        (items.iter()).find_map(|(headless, why)| headless.is(item).then_some(*why))
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

    /// The PRs whose Conversation the session read named no head, which the session adds to and takes from.
    pub(crate) fn headless(&self) -> Headless {
        self.headless.clone()
    }

    /// Tells the session to finish.
    pub(crate) fn stop_session(&self) {
        self.to_session(SessionMessage::Stop);
    }

    /// The user opened `repository`: the session loads the user's work and its Filters. A session with no
    /// token or no database has already shown why on the status bar, so this reports no failure.
    pub fn sync(&self, repository: RepositoryName) {
        self.to_session(SessionMessage::Sync { repository });
    }

    /// The user asked to refresh the open Repository now. A sync already under way covers this.
    pub fn refresh(&self) {
        self.to_session(SessionMessage::Refresh);
    }

    /// The settings owner saved a Repository's settings otherwise, and its view already says so. The Mirror
    /// worker and the read ahead follow the move of Reach from the settings the save replaced, whether or
    /// not the session has opened the Repository since launch. Which Repository is open stays, and no
    /// refresh starts.
    pub fn settings_changed(&self, save: SettingsSave) {
        self.to_session(SessionMessage::SettingsChanged(save));
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

    /// The user did `moved` with an item on screen, or with the window: the session reads each item the user
    /// can see from GitHub again a minute after its last read.
    pub(crate) fn screen_moved(&self, moved: ScreenMove) {
        self.to_session(SessionMessage::Screen(moved));
    }

    /// The reason the session's last read of PR `number`'s Conversation since launch left it with no head,
    /// or nothing when it found the head, or a demand has asked for another read since. A Conversation saved
    /// by an older Tsugiki, which kept no head, hasn't been read yet.
    pub(crate) fn why_no_head(&self, repository: &RepositoryName, number: u32) -> Option<NoHead> {
        self.headless.why(&Item {
            repository: repository.clone(),
            number,
        })
    }

    /// PR `number`'s Conversation named no head, its read failed, or the facts hold no such PR, so nothing
    /// will answer the Mirror worker's await of its head.
    pub(crate) fn no_head(&self, repository: RepositoryName, number: u32) {
        self.mirrors.no_head(repository, number);
    }

    /// The GitHub thread saved facts of `change`'s Repository that can change what the rules want of the
    /// Mirror, and of the items it names.
    pub(crate) fn facts_saved(&self, change: Change) {
        self.to_session(SessionMessage::Saved(change));
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

    /// Mute author on `author` in `repository` (#494): their items count as Muted, and the session marks
    /// their open items seen in the background, finding them under the Repository's settings.
    pub fn mute_author(&self, repository: RepositoryName, author: Login) {
        let muted = Marked::AuthorMuted {
            repository,
            author,
            at: OffsetDateTime::now_utc(),
        };
        self.to_session(SessionMessage::Mark(muted));
    }

    /// Unmutes `author` in `repository`.
    pub fn unmute_author(&self, repository: RepositoryName, author: Login) {
        let unmuted = Marked::AuthorUnmuted { repository, author };
        self.to_session(SessionMessage::Mark(unmuted));
    }

    /// Turns "Show muted" on the Inbox of `repository` to `show` (#825), and waits for the session to save
    /// it. A session that has stopped, or has no database, answers that it isn't connected.
    pub fn switch_show_muted(
        &self,
        repository: RepositoryName,
        show: ShowMuted,
    ) -> Result<(), Error> {
        self.answered(|reply| SessionMessage::ShowMuted {
            repository,
            show,
            reply,
        })
    }

    /// Fetches Issue or PR `number` of `repository`, which the database lacks, and waits for the answer.
    /// The GitHub thread takes it between two sync steps, ahead of the sync's queued work, saves what it
    /// found, and says which kind it was. A session with no token or no database answers
    /// `ItemUnfetched::Offline`, and has already shown why on the status bar. So does one offline or below
    /// the rate limit's floor, at once, without waiting for the request under way.
    pub fn fetch_unstored(&self, repository: RepositoryName, number: u32) -> ItemFetch {
        let answer = self.fetch_unstored_later(Item { repository, number });
        answer.recv().unwrap_or(ItemFetch::Unfetched {
            why: ItemUnfetched::Offline,
        })
    }

    /// Asks the session for `item`, which the database lacks, without waiting: the answer comes on the
    /// receiver. A session that has stopped drops the message with its reply, so the receiver gets none.
    pub(crate) fn fetch_unstored_later(&self, item: Item) -> Receiver<ItemFetch> {
        let (reply, answer) = mpsc::channel();
        self.to_session(SessionMessage::FetchUnstored { item, reply });
        answer
    }

    /// Asks GitHub what adding `name` would load. A session with no token or no database answers with an
    /// error, and has already shown why on the status bar.
    pub fn check(&self, name: RepositoryName) -> Result<RepositoryCheck, Error> {
        self.answered(|reply| SessionMessage::Check { name, reply })
    }

    /// Sends the session the message `asking` makes around a reply channel, and waits for its answer. A
    /// session that has stopped, or drops the message with its reply, answers that it isn't connected.
    fn answered<T>(
        &self,
        asking: impl FnOnce(Sender<Result<T, Error>>) -> SessionMessage,
    ) -> Result<T, Error> {
        let (reply, answer) = mpsc::channel();
        (self.session.send(asking(reply)).ok())
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
            tell(&session, patched);
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
        tell(&self.session, message);
    }
}

/// Sends the session `message`.
fn tell(session: &Sender<SessionMessage>, message: SessionMessage) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "this fails only once the session has stopped, or has shown on the status bar why it can't \
                  ask GitHub, and then nothing is left to hear the message"
    )]
    let _ = session.send(message);
}

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
