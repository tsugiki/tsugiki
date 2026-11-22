//! The one handle the app reads and fetches through, [`Calls`], with a call for each screen and a method for
//! each of the user's actions (ADR 0023). A screen's call reads what the core holds through a view model in
//! `workbench`, whose read sends nothing. A call whose read can find data missing queues through `fetching` a
//! job for what it lacks, and answers. An answer that says its data is on the way holds the job that brings
//! it. The app can name neither `fetching` nor the store's reader, so it runs no query and sends no demand.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::definitions::{
    Answer, Code, Definitions, HitLines, Lookup, OnIndex, OnWaiting, ServerName, ServerRow, Ticket,
    server_log,
};
use crate::diff::{DiffOf, DiffRow, Diffs, Found, HeadLine, Highlighter};
use crate::disk::FolderSize;
use crate::error::{Error, ErrorKind};
use crate::facts::CommitSha;
use crate::facts::activity::{ItemKey, ShowMuted};
use crate::facts::pull_requests::HeadInMirror;
use crate::fetching::{Fetched, Fetching, Item, NoHead, ScreenMove};
pub use crate::fetching::{ItemFetch, ItemUnfetched, Queued};
use crate::folders::Folders;
use crate::highlight::Span;
use crate::latest::LatestReader;
use crate::login::Login;
use crate::mirror::{CloneRefusal, LookupAt};
use crate::mirror_source::MirrorSource;
use crate::read_only::{Connection, ReadOnly, RepositoryState};
use crate::repository::RepositoryName;
use crate::repository_settings::{MirrorCost, NameCheck, RepositorySettings};
use crate::session::Session;
use crate::settings::{Home, LoadedSettings, SettingsChanges, SettingsOwner, SettingsView};
use crate::status::{ItemUnread, Shown};
use crate::store::StoreReader;
use crate::work::SeenMark;
use crate::workbench::inbox::{Inbox, InboxPage};
use crate::workbench::issue_view::IssueView;
use crate::workbench::pr_files::{
    DiffSources, DiffThread, FileLine, FilesRead, LineInDiff, OlderDiff, ShownDiff,
};
use crate::workbench::pr_view::PrView;
use crate::workbench::quick_open::{self, QuickOpenFound};
use crate::workbench::{Workbench, WorkbenchView};

/// The handle the app calls the core through, which it keeps as Tauri's state. Each screen's call answers
/// what the screen shows. An action returns once the core has queued it, but for the checks, which wait for
/// GitHub or git, those that save a settings file first, and Show muted, which waits for the store. A clone
/// is another handle on the same threads.
#[derive(Clone)]
pub struct Calls {
    fetching: Fetching,
    reader: StoreReader,
    /// The status board's copy of whether GitHub can be asked now, which each Repository's state reads.
    shown: LatestReader<Shown>,
    settings: SettingsOwner,
    folders: Result<Folders, Error>,
    /// The diff on show, which the Files tab's rows come from.
    diffs: Arc<Diffs>,
    highlighter: Highlighter,
    /// The language servers' thread, which the Definition peek asks.
    definitions: Definitions,
}

/// The parts of [`Calls`] that the session doesn't give: where the Mirrors are, the settings, and the
/// language servers.
pub struct CallParts {
    pub folders: Result<Folders, Error>,
    pub settings: SettingsOwner,
    pub definitions: Definitions,
}

impl Calls {
    /// The handle on `session` and `parts`, with a Highlighter of its own, once it has started what the
    /// window opens on. Make one per run: each starts the syncs and the clones again.
    pub fn new(session: &Session, parts: CallParts) -> Self {
        Self::launched(session.fetching(), session.reader(), session.shown(), parts)
    }

    /// The handle on `fetching`, `reader`, `shown` and `parts`, once it has started what the window opens
    /// on: the open Repository's sync, and its Mirror's clone ahead of the others. From here the settings
    /// owner hands the session each Repository's settings it saves otherwise, before the window can ask for
    /// a change. The Repositories added before 0.2 have no Mirror yet, and get theirs after the open one's.
    /// One the user hasn't said where its objects come from asks when its turn comes.
    fn launched(
        fetching: Fetching,
        reader: StoreReader,
        shown: LatestReader<Shown>,
        parts: CallParts,
    ) -> Self {
        let CallParts {
            folders,
            settings,
            definitions,
        } = parts;
        let calls = Self {
            fetching,
            reader,
            shown,
            settings,
            folders,
            diffs: Arc::default(),
            highlighter: Highlighter::start(),
            definitions,
        };
        let view = calls.settings.view();
        calls.start(&view);
        let fetching = calls.fetching.clone();
        calls
            .settings
            .tell_session(Box::new(move |save| fetching.settings_changed(save)));
        for repository in &view.loaded.settings.repositories {
            let source = view.mirror_sources.get(&repository.name).cloned();
            calls.fetching.clone_mirror(repository.name.clone(), source);
        }
        calls
    }

    /// Opens the Repository the window launches on or the user added, with its Mirror's objects from where
    /// the user said, if they have.
    fn start(&self, view: &SettingsView) {
        if let Home::Repository { repository, .. } = &view.home {
            let source = view.mirror_sources.get(&repository.name).cloned();
            self.open(&repository.name, source);
        }
    }

    /// Syncs `repository`, under the name the window knows it by, so the window recognises the changes. It
    /// takes every other sync step and the timed refresh from the one open before (ADR 0015), whose sync, if
    /// unfinished, goes on in its turns. Its Mirror's clone goes ahead of the others waiting, after the one
    /// under way, and doesn't wait on the sync. While neither `source` nor the Mirror worker holds an answer
    /// of where its objects come from, the clone waits for one.
    fn open(&self, repository: &RepositoryName, source: Option<MirrorSource>) {
        self.fetching.sync(repository.clone());
        self.fetching.clone_mirror_next(repository.clone(), source);
    }

    /// The settings of Repository `name`, or the defaults for one the settings don't list.
    fn repository(&self, name: RepositoryName) -> RepositorySettings {
        self.settings.view().repository(name)
    }

    /// The rows of a view of `repository`'s Workbench, with what its Sidebar shows, in the order
    /// `machine.toml` keeps for the list (#592).
    pub fn workbench(
        &self,
        repository: RepositoryName,
        view: &WorkbenchView,
    ) -> Result<Workbench, Error> {
        let settings = self.settings.view();
        let repository = settings.repository(repository);
        Workbench::read(&self.reader, &repository, view, settings.panes.order)
    }

    /// Page `page` of `repository`'s Inbox, as of now.
    pub fn inbox(&self, repository: RepositoryName, page: InboxPage) -> Result<Inbox, Error> {
        Inbox::read_now(&self.reader, &self.repository(repository), page)
    }

    /// The Issue view of Issue `number`, or nothing if Tsugiki doesn't hold it.
    pub fn issue(
        &self,
        repository: RepositoryName,
        number: u32,
    ) -> Result<Option<IssueView>, Error> {
        IssueView::read(&self.reader, &self.repository(repository), number)
    }

    /// The PR view of PR `number`, or nothing if Tsugiki doesn't hold it.
    pub fn pull_request(
        &self,
        repository: RepositoryName,
        number: u32,
    ) -> Result<Option<PrView>, Error> {
        PrView::read(&self.reader, &self.repository(repository), number)
    }

    /// The state of `repository`, open or not, which each write control reads: why Tsugiki can't write
    /// there, if it can't, and whether GitHub can be reached (#398).
    pub fn repository_state(&self, repository: &RepositoryName) -> Result<RepositoryState, Error> {
        let token = self
            .reader
            .token_cant_write(repository)?
            .then_some(ReadOnly::Token);
        let connection = if self.shown.read(|shown| shown.status.is_offline()) {
            Connection::Offline
        } else {
            Connection::Online
        };
        Ok(RepositoryState::new(token, connection))
    }

    /// The Issues and PRs of `repository` that quick open finds for what the user `typed`.
    pub fn quick_open(
        &self,
        repository: &RepositoryName,
        typed: &str,
    ) -> Result<QuickOpenFound, Error> {
        quick_open::find(&self.reader, repository, typed)
    }

    fn sources(&self) -> Result<DiffSources<'_>, Error> {
        let folders = self.folders()?;
        Ok(DiffSources {
            reader: &self.reader,
            folders,
            diffs: &self.diffs,
            highlighter: &self.highlighter,
        })
    }

    fn folders(&self) -> Result<&Folders, Error> {
        self.folders.as_ref().map_err(Clone::clone)
    }

    /// What the calls for a PR's screens read from and ask.
    fn pull_request_work(&self) -> PullRequestWork<'_> {
        PullRequestWork {
            sources: self.sources(),
            settings: self.settings.reader(),
            fetching: &self.fetching,
            definitions: &self.definitions,
        }
    }

    /// The user opened PR `number`: the session reads its Conversation from GitHub, each time, and again each
    /// minute while the user can see it, and the Mirror worker fetches its head first if it lacks it. It
    /// waits for one git, which says whether the mirror holds the head. The Conversation is read even when
    /// the mirrors' folder can't be found. Its Last seen stays.
    pub fn open_pull_request(&self, repository: &RepositoryName, number: u32) -> Result<(), Error> {
        self.pull_request_work()
            .open_pull_request(repository, number)
    }

    /// What PR `number`'s Files tab shows, or nothing if Tsugiki doesn't hold it, once the job that brings
    /// what the read found missing is queued. A diff that isn't on show waits on git, and never on GitHub.
    pub fn pr_files(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<PrFiles>, Error> {
        self.pull_request_work().pr_files(repository, number)
    }

    /// Rows `rows` of the diff `of` names.
    pub fn pr_rows(&self, of: &DiffOf, rows: Range<u32>) -> Result<Vec<DiffRow>, Error> {
        self.sources()?.rows(of, rows)
    }

    /// The spans of rows `rows` of the diff `of` names, but those of the files `collapsed` names.
    pub fn pr_spans(
        &self,
        of: &DiffOf,
        rows: Range<u32>,
        collapsed: &[u32],
    ) -> Result<Vec<Vec<Span>>, Error> {
        self.sources()?.spans(of, rows, collapsed)
    }

    /// Where the code of the diff `of` names holds `query`.
    pub fn pr_find(&self, of: &DiffOf, query: &str) -> Result<Found, Error> {
        self.sources()?.find(of, query)
    }

    /// Lines `lines` of file `file` of the diff `of` names, at its head.
    pub fn pr_lines(
        &self,
        of: &DiffOf,
        file: u32,
        lines: Range<u32>,
    ) -> Result<Vec<HeadLine>, Error> {
        self.sources()?.lines(of, file, lines)
    }

    /// The row of the diff `of` names that shows `line`, or else the line at the head.
    pub fn pr_row(&self, of: &DiffOf, line: &FileLine) -> Result<Option<LineInDiff>, Error> {
        self.sources()?.row(of, line)
    }

    /// Where `lookup`'s word is defined. `hearing` holds the listeners to what the lookup waits for, and to
    /// how the index of a server that answers while it builds it goes.
    pub fn look_up(
        &self,
        lookup: Lookup,
        ticket: Ticket,
        hearing: (OnWaiting, OnIndex),
    ) -> Result<Answer, Error> {
        self.definitions.look_up(lookup, ticket, hearing)
    }

    /// Cancels the lookup with `ticket`.
    pub fn cancel_definitions(&self, ticket: Ticket) {
        self.definitions.cancel(ticket);
    }

    /// The lines `wanted` names of a hit's file, from `repository`'s Mirror.
    pub fn definition_code(
        &self,
        repository: &RepositoryName,
        wanted: &HitLines,
    ) -> Result<Code, Error> {
        self.definitions.code(&self.mirror(repository)?, wanted)
    }

    /// The spans of the lines [`Calls::definition_code`] reads.
    pub fn definition_spans(
        &self,
        repository: &RepositoryName,
        wanted: &HitLines,
    ) -> Result<Vec<Vec<Span>>, Error> {
        let mirror = self.mirror(repository)?;
        (self.definitions).spans(&self.highlighter, &mirror, wanted)
    }

    fn mirror(&self, repository: &RepositoryName) -> Result<PathBuf, Error> {
        Ok(self.folders()?.mirror(repository))
    }

    /// Each language server's row in Settings › Advanced, as the servers' thread last wrote them.
    pub fn language_servers(&self) -> Vec<ServerRow> {
        self.definitions.rows()
    }

    /// The log that language server `server` writes for `repository`, which lives beside its Lookup
    /// checkout, once the server has written one.
    pub fn server_log(
        &self,
        repository: &RepositoryName,
        server: ServerName,
    ) -> Result<PathBuf, Error> {
        let log = server_log(&self.folders()?.lookup(repository), server);
        if !log.is_file() {
            let message = format!(
                "{} has written no log for {repository} yet.",
                server.as_str()
            );
            return Err(Error::new(ErrorKind::NotFound, &message));
        }
        Ok(log)
    }

    /// Reads both settings files again, and answers with the settings in force.
    pub fn reread_settings(&self) -> Result<LoadedSettings, Error> {
        self.in_force(self.settings.reread())
    }

    /// Saves what Settings' Apply changed, and answers with the settings in force after.
    pub fn apply_settings(&self, changes: SettingsChanges) -> Result<LoadedSettings, Error> {
        self.in_force(self.settings.apply(changes))
    }

    /// The settings in force once `settings` was read or applied. The language servers' thread hears they
    /// may have changed either way, as a failed read leaves the last settings in force.
    fn in_force(&self, settings: Result<SettingsView, Error>) -> Result<LoadedSettings, Error> {
        self.definitions.reconfigured();
        settings.map(|view| view.loaded)
    }

    /// Refreshes the open Repository now.
    pub fn refresh(&self) {
        self.fetching.refresh();
    }

    /// The user opened Issue `number`: the session reads what it says from GitHub now, and again each minute
    /// while the user can see it. Its Last seen stays: only Mark Seen moves it.
    pub fn open_issue(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.fetching.fetch_issue(item.repository.clone(), number);
        (self.fetching).screen_moved(ScreenMove::Opened(Fetched::Issue(item)));
    }

    /// The user asked for Issue or PR `number`, which the database lacks (#981): the session fetches it
    /// ahead of the sync's queued work and saves it, and this waits for the answer, which says whether it
    /// was an Issue or a PR, or why nothing came. The window then opens it as it opens any other item.
    pub fn fetch_item(&self, repository: RepositoryName, number: u32) -> ItemFetch {
        self.fetching.fetch_unstored(repository, number)
    }

    /// Another view hides the Issue or PR `number` the user opened, as a PR from an Issue's tree hides the
    /// Issue's: the session stops reading it each minute until it shows.
    pub fn hide_item(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.fetching.screen_moved(ScreenMove::Hid(item));
    }

    /// The Issue or PR `number` shows again: the session reads it at once if its minute came round while it
    /// was hidden.
    pub fn show_item(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.fetching.screen_moved(ScreenMove::Showed(item));
    }

    /// The user left the Issue or PR `number`, whose view is gone: the session reads it each minute no more.
    pub fn close_item(&self, repository: RepositoryName, number: u32) {
        let item = Item { repository, number };
        self.fetching.screen_moved(ScreenMove::Closed(item));
    }

    /// The window hid, as when the user minimised it: the session reads no item on screen until it shows.
    pub fn window_hidden(&self) {
        self.fetching.screen_moved(ScreenMove::WindowHid);
    }

    /// The window shows again: each item on screen whose minute came round meanwhile reads at once.
    pub fn window_shown(&self) {
        self.fetching.screen_moved(ScreenMove::WindowShowed);
    }

    /// The window loaded afresh, and shows: the session reads none of the items the page before had open.
    pub fn window_loaded(&self) {
        self.fetching.screen_moved(ScreenMove::WindowLoaded);
    }

    /// Mark Seen on what `mark` names.
    pub fn mark_seen(&self, repository: RepositoryName, mark: SeenMark) {
        self.fetching.mark_seen(repository, mark);
    }

    /// Mark Unseen on Issue or PR `number`, which unarchives `archived`, the item that holds it.
    pub fn mark_unseen(&self, repository: RepositoryName, number: u32, archived: Option<ItemKey>) {
        self.fetching.mark_unseen(repository, number, archived);
    }

    /// Archive on the item `item` names, with Mark Seen on what `mark` names.
    pub fn archive(&self, repository: RepositoryName, item: ItemKey, mark: SeenMark) {
        self.fetching.archive(repository, item, mark);
    }

    /// Mark Seen with `mark` on the item `item` names, which stays Archived.
    pub fn keep_archived(&self, repository: RepositoryName, item: ItemKey, mark: SeenMark) {
        self.fetching.keep_archived(repository, item, mark);
    }

    /// Unarchives the item `item` names.
    pub fn unarchive(&self, repository: RepositoryName, item: ItemKey) {
        self.fetching.unarchive(repository, item);
    }

    /// Mute on the item `item` names, with Mark Seen on what `mark` names.
    pub fn mute(&self, repository: RepositoryName, item: ItemKey, mark: SeenMark) {
        self.fetching.mute(repository, item, mark);
    }

    /// Unmutes the item `item` names.
    pub fn unmute(&self, repository: RepositoryName, item: ItemKey) {
        self.fetching.unmute(repository, item);
    }

    /// Mute author on `author` in `repository`.
    pub fn mute_author(&self, repository: RepositoryName, author: Login) {
        self.fetching.mute_author(repository, author);
    }

    /// Unmutes `author` in `repository`.
    pub fn unmute_author(&self, repository: RepositoryName, author: Login) {
        self.fetching.unmute_author(repository, author);
    }

    /// Pins Issue `number` of `repository` (#488).
    pub fn pin(&self, repository: RepositoryName, number: u32) {
        self.fetching.pin(repository, number);
    }

    /// Unpins Issue `number` of `repository` (#488).
    pub fn unpin(&self, repository: RepositoryName, number: u32) {
        self.fetching.unpin(repository, number);
    }

    /// Turns "Show muted" on the Inbox of `repository` to `show` (#825), once the store has kept it.
    pub fn switch_show_muted(
        &self,
        repository: RepositoryName,
        show: ShowMuted,
    ) -> Result<(), Error> {
        self.fetching.switch_show_muted(repository, show)
    }

    /// Says the Repository the user typed is one Tsugiki has, or else asks GitHub what adding it would load,
    /// and waits for its answer.
    pub fn check_repository(&self, typed: RepositoryName) -> Result<NameCheck, Error> {
        (self.settings).check_name(typed, &mut |name| self.fetching.check(name))
    }

    /// Asks GitHub what the Mirror of `repository` takes, with the check Add repository makes, and waits for
    /// its answer.
    pub fn mirror_cost(&self, repository: RepositoryName) -> Result<MirrorCost, Error> {
        Ok(self.fetching.check(repository)?.mirror)
    }

    /// Adds `repository`, starts its first sync (ADR 0015) and its Mirror's clone with its objects from
    /// `source`, and says what the window opens on now.
    pub fn add_repository(
        &self,
        repository: RepositoryName,
        source: MirrorSource,
    ) -> Result<Home, Error> {
        let view = self.settings.add_repository(repository, source)?;
        self.start(&view);
        Ok(view.home)
    }

    /// Makes `repository` the open one, and hands it to the session and the Mirror worker. It hands the
    /// worker no answer of where its objects come from: it has each one saved at launch, and each the user
    /// gave since, which `machine.toml` may have failed to keep.
    pub fn open_repository(&self, repository: RepositoryName) -> Result<Home, Error> {
        let view = self.settings.open_repository(repository)?;
        if let Home::Repository { repository, .. } = &view.home {
            self.open(&repository.name, None);
        }
        Ok(view.home)
    }

    /// Says why the clone in `folder` can't lend its objects to `repository`'s Mirror, or nothing if it can.
    /// It waits for git commands that only read the clone.
    pub fn check_clone(&self, folder: &Path, repository: &RepositoryName) -> Option<CloneRefusal> {
        self.fetching.check_clone(folder, repository).err()
    }

    /// Starts `repository`'s clone ahead of the others, with its objects from `source`, and saves the answer
    /// in `machine.toml`. The clone goes ahead even if `machine.toml` can't keep it: the next launch asks
    /// again.
    pub fn choose_mirror_source(
        &self,
        repository: RepositoryName,
        source: MirrorSource,
    ) -> Result<(), Error> {
        self.fetching
            .clone_mirror_next(repository.clone(), Some(source.clone()));
        drop(self.settings.save_mirror_source(repository, source)?);
        Ok(())
    }

    /// The Mirror worker looks at git and the disk again, and tries the clones that waited for them.
    pub fn check_mirrors_again(&self) {
        self.fetching.check_mirrors_again();
    }

    /// What each Repository the settings list has on this machine, in their order, for Settings ›
    /// Repositories (#533). It measures each Mirror on the disk, so the window asks off its own thread.
    pub fn on_this_machine(&self) -> Result<Vec<OnThisMachine>, Error> {
        Ok(on_this_machine(&self.settings.view(), self.folders()?))
    }
}

/// What a Repository has on this machine, which `machine.toml` and the disk hold.
#[derive(Debug, PartialEq, Eq, Serialize, Type)]
pub struct OnThisMachine {
    pub repository: RepositoryName,
    /// Where its Mirror gets its objects, or nothing if the user hasn't said.
    pub source: Option<MirrorSource>,
    pub mirror: FolderSize,
}

/// What each Repository `view` lists has on this machine, with its Mirror where `folders` keeps it.
fn on_this_machine(view: &SettingsView, folders: &Folders) -> Vec<OnThisMachine> {
    (view.loaded.settings.repositories.iter())
        .map(|repository| OnThisMachine {
            repository: repository.name.clone(),
            source: view.mirror_sources.get(&repository.name).cloned(),
            mirror: FolderSize::of(&folders.mirror(&repository.name)),
        })
        .collect()
}

/// What the Files tab shows.
#[derive(Debug, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PrFiles {
    /// The Repository's mirror is still being cloned, and the clone says how far it is.
    Cloning {
        #[serde(skip)]
        #[specta(skip)]
        clone: Queued,
    },
    /// The Repository's mirror isn't cloned yet, and its clone waits for the user to say where it gets its
    /// objects.
    Asking,
    /// The mirror lacks the PR's commits, and is fetching them.
    Fetching {
        #[serde(skip)]
        #[specta(skip)]
        fetch: Queued,
    },
    /// Tsugiki doesn't know the PR's head, and is reading the Conversation that names it.
    FindingHead {
        #[serde(skip)]
        #[specta(skip)]
        fetch: Queued,
    },
    /// Tsugiki doesn't know the PR's head, and the read of the Conversation that names it failed for `why`.
    Unread { why: ItemUnread },
    /// The diff, whose rows the window asks for by number.
    Ready {
        #[serde(flatten)]
        diff: ShownDiff,
        threads: DiffThreads,
    },
}

/// The review threads the Files tab shows with a diff.
#[derive(Debug, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffThreads {
    /// The threads, in the order the tab shows them, from the PR's Conversation as Tsugiki last read it.
    Shown { threads: Vec<DiffThread> },
    /// The PR has moved on to `newer`, which the mirror is fetching, and the diff is the one before. The
    /// threads' lines count in `newer`, so the tab holds back all `count` of them until its diff comes.
    HeldBack {
        #[specta(type = String)]
        newer: CommitSha,
        count: u32,
        #[serde(skip)]
        #[specta(skip)]
        fetch: Queued,
    },
}

/// What the calls of a PR's screens read from and ask: where the Files tab reads, or why it can't, the
/// settings, which list the Repositories Tsugiki keeps a mirror of, the queues behind `fetching`, and the
/// language servers, which open the files of the diff on show.
struct PullRequestWork<'a> {
    sources: Result<DiffSources<'a>, Error>,
    settings: LatestReader<SettingsView>,
    fetching: &'a Fetching,
    definitions: &'a Definitions,
}

impl PullRequestWork<'_> {
    /// The user opened PR `number`: the session reads its Conversation from GitHub, each time, and again each
    /// minute while the user can see it, and the Mirror worker fetches its head first if it lacks it. It
    /// waits for one git, which says whether the mirror holds the head. The Conversation is read even when
    /// the mirrors' folder can't be found. Its Last seen stays.
    fn open_pull_request(&self, repository: &RepositoryName, number: u32) -> Result<(), Error> {
        self.fetching.fetch_pull_request(repository.clone(), number);
        let item = Item {
            repository: repository.clone(),
            number,
        };
        (self.fetching).screen_moved(ScreenMove::Opened(Fetched::PullRequest(item)));
        if let Some(head) = self.sources()?.head_in_mirror(repository, number)? {
            self.fetching
                .pull_request_opened(repository.clone(), number, head);
        }
        Ok(())
    }

    /// What PR `number`'s Files tab shows, or nothing if Tsugiki doesn't hold it, once the job that brings
    /// what the read found missing is queued. A diff that isn't on show waits on git, and never on GitHub.
    fn pr_files(&self, repository: &RepositoryName, number: u32) -> Result<Option<PrFiles>, Error> {
        let asks = || self.fetching.mirror_asks(repository);
        let read = self.sources()?.read(repository, number, &asks)?;
        read.map(|read| self.answer(repository, number, read))
            .transpose()
    }

    fn sources(&self) -> Result<&DiffSources<'_>, Error> {
        self.sources.as_ref().map_err(Clone::clone)
    }

    /// What the Files tab of PR `number` shows of `read`. Each answer that waits for data queues the job
    /// that brings it. A Repository the settings don't list has no mirror to wait for.
    fn answer(
        &self,
        repository: &RepositoryName,
        number: u32,
        read: FilesRead,
    ) -> Result<PrFiles, Error> {
        let fetching = self.fetching;
        Ok(match read {
            FilesRead::Asking => PrFiles::Asking,
            FilesRead::NoMirror => {
                let settings = self.settings.get().loaded.settings;
                let listed =
                    (settings.repositories.iter()).any(|listed| listed.name.is_same(repository));
                if !listed {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        &format!(
                            "Tsugiki keeps no mirror of {repository}, which the settings don't list."
                        ),
                    ));
                }
                PrFiles::Cloning {
                    clone: fetching.clone_shown(repository.clone()),
                }
            }
            FilesRead::NoHead => match fetching.why_no_head(repository, number) {
                // A read GitHub answered named no head, so another would bring the same.
                Some(NoHead::Unnamed) => {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        &format!(
                            "GitHub named no head commit of PR #{number} when Tsugiki read its \
                             Conversation, so it has no diff to show."
                        ),
                    ));
                }
                // The session reads the PR on screen again each minute, as for the Conversation tab.
                Some(NoHead::Unread(why)) => PrFiles::Unread { why },
                None => PrFiles::FindingHead {
                    fetch: fetching.find_head(repository.clone(), number),
                },
            },
            FilesRead::Missing { head, older: None } => PrFiles::Fetching {
                fetch: fetching.fetch_head(repository.clone(), head),
            },
            FilesRead::Missing {
                head,
                older: Some(OlderDiff { diff, threads }),
            } => {
                let newer = head.commit.clone();
                let fetch = fetching.fetch_head(repository.clone(), head);
                let threads = DiffThreads::HeldBack {
                    newer,
                    count: threads,
                    fetch,
                };
                self.ready(repository, diff, threads)
            }
            FilesRead::Held { diff, threads } => {
                fetching.pull_request_opened(repository.clone(), number, HeadInMirror::Held);
                self.ready(repository, diff, DiffThreads::Shown { threads })
            }
        })
    }

    /// The diff on show is the PR on screen: its Lookup checkout moves to the diff's ends, where text search
    /// looks for them, and ty opens its Python files.
    fn ready(&self, repository: &RepositoryName, diff: ShownDiff, threads: DiffThreads) -> PrFiles {
        let at = LookupAt {
            base: diff.base.clone(),
            head: diff.head.clone(),
        };
        self.fetching.look_up(repository.clone(), at.clone());
        self.definitions.opened(repository.clone(), at, &diff.files);
        PrFiles::Ready { diff, threads }
    }
}

#[cfg(test)]
mod tests;
