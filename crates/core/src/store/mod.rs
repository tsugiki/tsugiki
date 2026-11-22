//! Tsugiki keeps what it learns in one SQLite file (ADR 0010). `Store` is the one writer, and `StoreReader`
//! is a read-only connection for screens, so a screen never waits on a sync (ADR 0018). Each connection has
//! a thread that runs its jobs in turn, so a caller waits only for the jobs ahead of its own on that
//! connection.

mod activity;
mod columns;
mod failed_checks;
mod file;
mod filters;
mod issue_content;
mod issues;
mod migrate;
mod patch_ids;
mod pr_links;
mod pull_request_content;
mod pull_requests;
mod read_only;
/// Draws the schema in `docs/schema.md` and keeps it current. Only its tests use it.
#[cfg(test)]
mod schema;
mod sync_state;

pub use activity::{Archiving, AuthorMuting, Flagging, Judged, Muting, Opened, Seeing};

use pull_requests::FirstSave;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::{OpenFlags, OptionalExtension, Transaction, named_params};
use serde::Serialize;
use specta::Type;
use time::OffsetDateTime;

use crate::conventions::Conventions;
use crate::error::{Error, ErrorKind};
use crate::facts::activity::{ActivityFacts, ItemActivity, ItemKey, ShowMuted};
use crate::facts::filters::FilterResults;
use crate::facts::issue_content::IssueContent;
use crate::facts::issues::{IssueFacts, IssuePlace, IssueState};
use crate::facts::patch_ids::{CommitPatches, Findings, LastCheck, ReviewFacts, UncheckedCommit};
use crate::facts::pull_request_content::PullRequestContent;
use crate::facts::pull_requests::{NodeId, PrLinks, PrState, PullRequestFacts, ReviewedCommit};
use crate::facts::{CommitSha, WorkFacts};
use crate::filters::FilterQuery;
use crate::login::Login;
use crate::read_only::TokenSeen;
use crate::repository::RepositoryName;
use crate::token::TokenKind;
use columns::Items;
pub use columns::Numbers;
pub use filters::NarrowedResults;
use migrate::{MIGRATIONS, Migration};
pub use pr_links::Updates;
pub use pull_request_content::{ConversationSaved, HeadKept};
pub use sync_state::{ChangeCheck, FirstSyncPlace, OVERLAP, SyncState};

/// The database connection, which only `store` may hold. Code here names it through this alias, so
/// Clippy's ban on `rusqlite::Connection` holds everywhere else, and its other bans hold here too.
#[expect(
    clippy::disallowed_types,
    reason = "store is the one module that holds a database connection, so its tables stay its own"
)]
type Connection = rusqlite::Connection;

const CANT_OPEN: &str = "Tsugiki can't open its database";
const CANT_READ: &str = "Tsugiki can't read its database";
const CANT_WRITE: &str = "Tsugiki can't write to its database";
const CANT_UPDATE: &str = "Tsugiki can't update its database to this version";
const NEWER: &str = "A newer version of Tsugiki wrote this database, so this version won't open it. \
                     Update Tsugiki to read it.";
pub(crate) const IN_USE: &str = "Another Tsugiki has this database open, so this one leaves it alone. \
                                 Use that one, or close it and start Tsugiki again.";
const STOPPED: &str = "Tsugiki's database stopped working. Restart Tsugiki to open it again.";

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// The open database.
pub struct Store {
    jobs: Sender<Job>,
    thread: JoinHandle<()>,
}

impl Store {
    /// Opens the database at `path`, creating it or moving it to the current schema as needed.
    pub fn open(path: &Path) -> Result<Self, Error> {
        open_with(path, MIGRATIONS)
    }

    /// The user signed in when Tsugiki last reached GitHub, if it ever has.
    pub fn last_login(&self) -> Result<Option<Login>, Error> {
        let found = self.call(|connection| read_last_login(connection))?;
        found.map_err(saying(CANT_READ))
    }

    pub fn remember_login(&self, login: &Login) -> Result<(), Error> {
        let login = login.clone();
        let written = self.call(move |connection| write_last_login(connection, &login))?;
        written.map_err(saying(CANT_WRITE))
    }

    /// Stores `pull_requests` of `repository` in one transaction (ADR 0010).
    pub fn save_pull_requests(
        &self,
        repository: &RepositoryName,
        pull_requests: Vec<PullRequestFacts>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        self.save_fetched_pull_requests(&repository, pull_requests, Vec::new())
    }

    /// Stores `pull_requests` and what GitHub says happened on them in one transaction, so a crash can't
    /// keep a PR's new `updatedAt` and lose the activity that a later sync then has no reason to ask for.
    pub fn save_fetched_pull_requests(
        &self,
        repository: &RepositoryName,
        pull_requests: Vec<PullRequestFacts>,
        events: Vec<ItemActivity>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            pull_requests::save(connection, &repository, &pull_requests, &events)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores a PR the user fetched by its number, which the store lacked (#981), as
    /// `save_fetched_pull_requests` does. A PR updated since the next refresh's poll starts, a little before
    /// the last sync of `repository` started, is one that refresh would find changed, so it may have opened
    /// since, as the refresh would judge it (#823). An older one, or one in a Repository that has finished
    /// no sync, was loaded from before.
    pub fn save_unstored_pull_request(
        &self,
        repository: &RepositoryName,
        (facts, activity): (PullRequestFacts, ItemActivity),
    ) -> Result<(), Error> {
        let (repository, pr) = (repository.clone(), (facts, activity));
        let saved = self.call(move |connection| save_unstored_in(connection, &repository, pr))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores PRs a refresh fetched or probed, as `save_fetched_pull_requests` does, and in the same
    /// transaction owes a read of the Conversation of each of `changed`. A refresh that stops after this has
    /// saved them can't tell they changed, so the next one reads them ahead from here (#205). A PR the store
    /// doesn't hold yet may have opened since the last refresh (#823).
    pub fn save_changed_pull_requests(
        &self,
        repository: &RepositoryName,
        (pull_requests, events): (Vec<PullRequestFacts>, Vec<ItemActivity>),
        changed: Vec<u32>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            let saving = (pull_requests.as_slice(), FirstSave::MayHaveOpened);
            pull_requests::save_in(&transaction, &repository, saving, &events)?;
            sync_state::owe_read_ahead(&transaction, &repository, &changed)?;
            transaction.commit()
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Owes a read of the Conversation of each of `numbers`, PRs of `repository`, until a read of it saves or
    /// a refresh finishes.
    pub fn owe_read_ahead(
        &self,
        repository: &RepositoryName,
        numbers: Vec<u32>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            sync_state::owe_read_ahead(connection, &repository, &numbers)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The PRs of `repository` whose Conversations the store owes a read. Saving a PR's Conversation pays
    /// its debt, and saving a finished refresh pays them all.
    pub fn read_ahead_owed(&self, repository: &RepositoryName) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| sync_state::read_ahead_owed(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The head each of `repository`'s held Conversations was read at, of those read with one.
    pub fn conversation_heads(
        &self,
        repository: &RepositoryName,
    ) -> Result<BTreeMap<u32, CommitSha>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| pull_request_content::read_heads(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Each PR Tsugiki has of `repository`, for a sync to see what it's missing.
    pub fn pull_requests(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<PullRequestFacts>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| pull_requests::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The Issues of `repository` saved before Tsugiki kept states, which a sync fetches once (#231).
    pub fn issues_without_state(&self, repository: &RepositoryName) -> Result<Vec<u32>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| issues::numbers_without_state(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The merged and closed PRs of `repository`, which only a refresh that sees one move fetches again
    /// (ADR 0011).
    pub fn frozen_pull_requests(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<PullRequestFacts>, Error> {
        let mut found = self.pull_requests(repository)?;
        found.retain(PullRequestFacts::is_frozen);
        Ok(found)
    }

    /// Each Issue Tsugiki has of `repository`, for a sync to see what it's missing.
    pub fn issues(&self, repository: &RepositoryName) -> Result<Vec<IssueFacts>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| issues::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Replaces what the search `query` found in `repository`, in one transaction (ADR 0010).
    pub fn save_filter_results(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        results: FilterResults,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let query = query.clone();
        let saved =
            self.call(move |connection| filters::save(connection, &repository, &query, &results))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Applies a narrowed search to what `query` found in `repository`, in one transaction (ADR 0010).
    /// Answers with the numbers that joined or left the Filter.
    pub fn merge_filter_results(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        narrowed: NarrowedResults,
    ) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let query = query.clone();
        let merge = move |connection: &mut Connection| {
            filters::merge(connection, &repository, &query, &narrowed)
        };
        let merged = self.call(merge)?;
        merged.map_err(saying(CANT_WRITE))
    }

    /// Each search of `repository` that has run, with its results, for a refresh to see how to ask again.
    pub fn filter_results(
        &self,
        repository: &RepositoryName,
    ) -> Result<BTreeMap<FilterQuery, FilterResults>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| filters::read_all(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The facts behind the Workbench of `repository`, as this writer last saved them.
    pub fn work_facts(&self, repository: &RepositoryName) -> Result<WorkFacts, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_work_facts(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The last sync of `repository` that finished, if one has.
    pub fn sync_state(&self, repository: &RepositoryName) -> Result<Option<SyncState>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| sync_state::read(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Saves where `repository`'s syncs stand: a refresh under way, or a first sync that finished. Neither
    /// has read ahead, so the reads the store owes stay owed for the next refresh (#900).
    pub fn save_sync_state(
        &self,
        repository: &RepositoryName,
        state: SyncState,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            save_sync_state_in(&transaction, &repository, &state)?;
            transaction.commit()
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Saves that a refresh of `repository` finished, as `save_sync_state` does, and in the same transaction
    /// forgets every read ahead the store owed: the refresh read each PR it owed, or found it out of Reach.
    pub fn save_finished_refresh(
        &self,
        repository: &RepositoryName,
        state: SyncState,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            let transaction = connection.transaction()?;
            save_sync_state_in(&transaction, &repository, &state)?;
            sync_state::forget_read_ahead(&transaction, &repository)?;
            transaction.commit()
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The place this Repository's first sync got to, if one stopped partway.
    pub fn first_sync_place(
        &self,
        repository: &RepositoryName,
    ) -> Result<Option<FirstSyncPlace>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| sync_state::read_place(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    pub fn save_first_sync_place(
        &self,
        repository: &RepositoryName,
        place: FirstSyncPlace,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| sync_state::save_place(connection, &repository, &place))?;
        saved.map_err(saying(CANT_WRITE))
    }

    #[cfg(test)]
    pub(crate) fn forget_sync_state(&self, repository: &RepositoryName) -> Result<(), Error> {
        let repository = repository.clone();
        let forgot = self.call(move |connection| sync_state::forget(connection, &repository))?;
        forgot.map_err(saying(CANT_WRITE))
    }

    /// Says what stopped the search `query`, and leaves what it last found.
    pub fn save_filter_problem(
        &self,
        repository: &RepositoryName,
        query: &FilterQuery,
        problem: &str,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let query = query.clone();
        let problem = problem.to_owned();
        let save = move |connection: &mut Connection| {
            filters::save_problem(connection, &repository, &query, &problem)
        };
        let saved = self.call(save)?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Removes the results of each search of `repository` outside `keep`, which the user has edited away.
    pub fn keep_only_filters(
        &self,
        repository: &RepositoryName,
        keep: Vec<FilterQuery>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| filters::keep_only(connection, &repository, &keep))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores `issues` of `repository` in one transaction (ADR 0010).
    pub fn save_issues(
        &self,
        repository: &RepositoryName,
        issues: Vec<IssueFacts>,
    ) -> Result<(), Error> {
        self.save_fetched_issues(repository, issues, Vec::new())
    }

    /// Stores `issues` and what GitHub says happened on them in one transaction, as
    /// `save_fetched_pull_requests` does.
    pub fn save_fetched_issues(
        &self,
        repository: &RepositoryName,
        issues: Vec<IssueFacts>,
        events: Vec<ItemActivity>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| issues::save(connection, &repository, &issues, &events))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Stores `issues` of `repository` and what GitHub says happened on them, and keeps each of `issueless`
    /// as a number that will never name an Issue, which `named_issues_lacking` leaves out from then on
    /// (#980). One transaction holds it all.
    pub fn save_issues_and_issueless(
        &self,
        repository: &RepositoryName,
        (issues, events): (Vec<IssueFacts>, Vec<ItemActivity>),
        issueless: Vec<u32>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let save = move |connection: &mut Connection| {
            issues::save_with_issueless(connection, &repository, (&issues, &events), &issueless)
        };
        let saved = self.call(save)?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The numbers the PRs of `repository` name as fixed under `conventions`, that Tsugiki holds as neither
    /// an Issue nor a PR, and that can still name an Issue (#980).
    pub fn named_issues_lacking(
        &self,
        repository: &RepositoryName,
        conventions: Conventions,
    ) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let read = move |connection: &mut Connection| {
            issues::named_and_lacking(connection, &repository, conventions)
        };
        let read = self.call(read)?;
        read.map_err(saying(CANT_READ))
    }

    /// The numbers of `viewer`'s open PRs whose CI failed after Tsugiki last asked which checks failed.
    pub fn pull_requests_with_unread_checks(
        &self,
        repository: &RepositoryName,
        viewer: &Login,
    ) -> Result<Vec<u32>, Error> {
        let repository = repository.clone();
        let viewer = viewer.clone();
        let read =
            self.call(move |connection| failed_checks::unread(connection, &repository, &viewer))?;
        read.map_err(saying(CANT_READ))
    }

    /// Replaces what the store knows about each PR's failed checks.
    pub fn save_failed_checks(
        &self,
        repository: &RepositoryName,
        read: Vec<(u32, Vec<String>)>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| failed_checks::save(connection, &repository, &read))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Replaces what's kept of what Issue `number` says.
    pub fn save_issue_content(
        &self,
        repository: &RepositoryName,
        number: u32,
        content: IssueContent,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            issue_content::save(connection, &repository, number, &content)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Keeps what PR `number` says, its Reviews included, in place of what was kept before. `read` pairs the
    /// content with GitHub's `updatedAt` for the PR at the read. A refresh no longer owes a read of it. The
    /// PR's facts keep the head the content names when that `updatedAt` is later than at the read whose head
    /// they keep.
    pub fn save_pull_request_content(
        &self,
        repository: &RepositoryName,
        number: u32,
        read: (PullRequestContent, OffsetDateTime),
    ) -> Result<(), Error> {
        let saved = self.save_read_of_pull_request(repository, number, read);
        saved.map(|_kept| ())
    }

    /// Keeps what PR `number` says as `save_pull_request_content` does, and says what that did to the head
    /// in the PR's facts and to the commit the user's latest Review covered.
    pub fn save_read_of_pull_request(
        &self,
        repository: &RepositoryName,
        number: u32,
        (content, updated_at): (PullRequestContent, OffsetDateTime),
    ) -> Result<ConversationSaved, Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            pull_request_content::save(connection, &repository, number, (&content, updated_at))
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The commit each of the user's Reviews in `repository` covered, for its mirror to keep. None before
    /// Tsugiki knows who the user is.
    pub fn reviewed_commits(
        &self,
        repository: &RepositoryName,
    ) -> Result<Vec<ReviewedCommit>, Error> {
        let repository = repository.clone();
        let reviewed = move |connection: &mut Connection| {
            pull_request_content::reviewed_commits(connection, &repository)
        };
        let read = self.call(reviewed)?;
        read.map_err(saying(CANT_READ))
    }

    /// Keeps the per-file patch-ids of each commit in `patches`, which the Stale review check compares.
    pub fn save_patch_ids(
        &self,
        repository: &RepositoryName,
        patches: Vec<CommitPatches>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| patch_ids::save(connection, &repository, &patches))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Keeps `unchecked` as the commits of `repository` whose patch-ids the Mirror worker can't read now, in
    /// place of those kept before.
    pub fn save_unchecked(
        &self,
        repository: &RepositoryName,
        unchecked: Vec<UncheckedCommit>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            patch_ids::save_unchecked(connection, &repository, &unchecked)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Forgets why the Mirror worker couldn't read patch-ids in a run that ended, as a new run starts.
    pub fn forget_unchecked(&self) -> Result<(), Error> {
        let forgot = self.call(|connection| patch_ids::forget_unchecked(connection))?;
        forgot.map_err(saying(CANT_WRITE))
    }

    /// The Repositories whose kept links rules other than `LinkRule::SET` worked out, which the session
    /// refills, or nothing while the set worked them out (#950).
    pub fn stale_links(&self) -> Result<Option<Vec<RepositoryName>>, Error> {
        let found = self.call(|connection| pr_links::stale(connection))?;
        found.map_err(saying(CANT_READ))
    }

    /// Works out the links of the first `most` PRs of `repository` from number `from` up again, under
    /// `LinkRule::SET`, in place of those kept. Answers with the number the next batch starts from, or
    /// nothing once the Repository has no PR left. A batch that fails keeps the links it would replace.
    pub fn refill_links(
        &self,
        repository: &RepositoryName,
        from: u32,
        most: std::num::NonZeroU32,
    ) -> Result<Option<u32>, Error> {
        let repository = repository.clone();
        let refilled =
            self.call(move |connection| pr_links::refill(connection, &repository, from, most))?;
        refilled.map_err(saying(CANT_WRITE))
    }

    /// Saves the names of `LinkRule::SET` as the rules that worked out the kept links, once the session has
    /// refilled those of each Repository `stale_links` named. The links of a Repository whose stored name no
    /// longer parses go with it.
    pub fn keep_link_rules(&self) -> Result<(), Error> {
        let kept = self.call(pr_links::refilled)?;
        kept.map_err(saying(CANT_WRITE))
    }

    /// Keeps each of `checks` as its PR's last check, whose verdict stands while a newer check waits.
    pub fn save_last_checks(
        &self,
        repository: &RepositoryName,
        checks: Vec<(u32, LastCheck)>,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            patch_ids::save_last_checks(connection, &repository, &checks)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Keeps `findings` of when checks first found Reviews stale, each found one dated `now`.
    pub fn save_findings(
        &self,
        repository: &RepositoryName,
        findings: Findings,
        now: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| {
            patch_ids::save_findings(connection, &repository, &findings, now)
        })?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Mark Seen on `seeing`'s items: clears their Marked unseen, moves their Last seen on to its `up_to`, if
    /// it's given, and acknowledges its findings, the Stale reviews the window drew on them (#908).
    pub fn mark_seen(&self, repository: &RepositoryName, seeing: Seeing) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| activity::mark_seen(connection, &repository, &seeing))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Flags `flagging`'s Issue or PR Marked unseen from `at`, and unarchives what it names.
    pub fn mark_unseen(
        &self,
        repository: &RepositoryName,
        flagging: Flagging,
        at: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| activity::mark_unseen(connection, &repository, flagging, at))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Archive on `archiving`'s item: Mark Seen on its numbers, and the item kept as archived from their
    /// `up_to`, or from `now` without one, as `activity::archive` says.
    pub fn archive(
        &self,
        repository: &RepositoryName,
        archiving: Archiving,
        now: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| activity::archive(connection, &repository, &archiving, now))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The Issues and PRs of `repository` a save found closed that the store held open, whose closing the
    /// sync hasn't judged yet (#820).
    pub fn closings(&self, repository: &RepositoryName) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| activity::closings(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Archives the items whose closing `judged` says archives them, put away `now`, and forgets the
    /// closings it judged, as `activity::archive_closed` says.
    pub fn archive_closed(
        &self,
        repository: &RepositoryName,
        judged: Judged,
        now: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let archive = move |connection: &mut Connection| {
            activity::archive_closed(connection, &repository, &judged, now)
        };
        let saved = self.call(archive)?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The PRs of `repository` a save stored for the first time that name an Archived Issue, whose opening
    /// the sync hasn't judged yet (#823).
    pub fn openings(&self, repository: &RepositoryName) -> Result<BTreeSet<u32>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| activity::openings(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// Brings back to the Inbox the Issues a PR opened on, as `opened` judges them, and forgets the openings
    /// it judged, as `activity::bring_back` says.
    pub fn bring_back(&self, repository: &RepositoryName, opened: Opened) -> Result<(), Error> {
        let repository = repository.clone();
        let bring = move |connection: &mut Connection| {
            activity::bring_back(connection, &repository, &opened)
        };
        let saved = self.call(bring)?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Unarchives the item `key` names.
    pub fn unarchive(&self, repository: &RepositoryName, key: ItemKey) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| activity::unarchive(connection, &repository, key))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Mute on `muting`'s item: Mark Seen on its numbers, and the item kept as muted, as `activity::mute`
    /// says.
    pub fn mute(
        &self,
        repository: &RepositoryName,
        muting: Muting,
        now: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved =
            self.call(move |connection| activity::mute(connection, &repository, &muting, now))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Unmutes the item `key` names.
    pub fn unmute(&self, repository: &RepositoryName, key: ItemKey) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| activity::unmute(connection, &repository, key))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Mute author on `muting`'s author: Mark Seen on the items it names, and the author kept as muted, as
    /// `activity::mute_author` says.
    pub fn mute_author(
        &self,
        repository: &RepositoryName,
        muting: AuthorMuting,
        now: OffsetDateTime,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| activity::mute_author(connection, &repository, &muting, now))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Unmutes `author`.
    pub fn unmute_author(&self, repository: &RepositoryName, author: &Login) -> Result<(), Error> {
        let (repository, author) = (repository.clone(), author.clone());
        let saved =
            self.call(move |connection| activity::unmute_author(connection, &repository, &author))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Forgets the Read-only marks of each token of another kind than `kind`, which Tsugiki knows before
    /// GitHub first answers, and gives the Repositories they were on (#398).
    pub fn forget_read_only_of_other_kinds(
        &self,
        kind: TokenKind,
    ) -> Result<Vec<RepositoryName>, Error> {
        let saved = self.call(move |connection| read_only::forget_other_kinds(connection, kind))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Forgets the Read-only marks of each token but `token`, and gives the Repositories they were on
    /// (#398).
    pub fn forget_read_only_of_other_tokens(
        &self,
        token: TokenSeen,
    ) -> Result<Vec<RepositoryName>, Error> {
        let saved =
            self.call(move |connection| read_only::forget_other_tokens(connection, token))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Marks `repository` Read-only, as `token` can't write to it, and gives whether it wasn't before
    /// (#398).
    pub fn mark_read_only(
        &self,
        token: TokenSeen,
        repository: &RepositoryName,
    ) -> Result<bool, Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| read_only::mark(connection, token, &repository))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Forgets the Read-only marks of each Repository not in `listed`, and gives the Repositories they
    /// were on (#398).
    pub fn forget_read_only_of_unlisted(
        &self,
        listed: Vec<RepositoryName>,
    ) -> Result<Vec<RepositoryName>, Error> {
        let saved = self.call(move |connection| read_only::forget_unlisted(connection, &listed))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Clears `repository`'s Read-only mark, and gives whether it had one (#398).
    pub fn clear_read_only(&self, repository: &RepositoryName) -> Result<bool, Error> {
        let repository = repository.clone();
        let saved = self.call(move |connection| read_only::clear(connection, &repository))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// Whether the store marks `repository` as one the token can't write to, as a write is about to go
    /// out (#914).
    pub fn token_cant_write(&self, repository: &RepositoryName) -> Result<bool, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_only::marked(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// GitHub's ID of Issue or PR `number` of `repository`, which a write names it by, from its last read.
    /// Nothing, before Tsugiki has read it (#914).
    pub fn node_id(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<NodeId>, Error> {
        let repository = repository.clone();
        let read =
            self.call(move |connection| issue_content::node_id(connection, &repository, number))?;
        read.map_err(saying(CANT_READ))
    }

    /// Turns `repository`'s "Show muted" to `show` (#825).
    pub fn switch_show_muted(
        &self,
        repository: &RepositoryName,
        show: ShowMuted,
    ) -> Result<(), Error> {
        let repository = repository.clone();
        let saved = self
            .call(move |connection| activity::switch_show_muted(connection, &repository, show))?;
        saved.map_err(saying(CANT_WRITE))
    }

    /// The events kept for item `number`, oldest first.
    #[cfg(test)]
    pub fn activity(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Vec<crate::facts::activity::Activity>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| activity::read(connection, &repository, number))?;
        read.map_err(saying(CANT_READ))
    }

    /// Finishes what was asked for and closes the file.
    pub fn close(self) {
        drop(self.jobs);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a thread that panicked has closed the file too, and `call` has already said so"
        )]
        let _ = self.thread.join();
    }

    /// Runs `work` on the thread that owns the connection, and waits for its answer.
    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> T + Send + 'static,
    ) -> Result<T, Error> {
        ask(&self.jobs, work)
    }

    /// This store, except that call number `lost`, if any, goes missing as if its thread had stopped. The
    /// other calls reach the database in order, so a step can save one thing and fail on the next. The
    /// receiver hears of each call.
    #[cfg(test)]
    pub(crate) fn failing_at_call(
        &self,
        lost: Option<usize>,
    ) -> (Self, std::sync::mpsc::Receiver<usize>) {
        let (jobs, calls) = std::sync::mpsc::channel::<Job>();
        let (seen, heard) = std::sync::mpsc::channel();
        let database = self.jobs.clone();
        let thread = std::thread::spawn(move || {
            for (call, job) in calls.iter().enumerate() {
                seen.send(call).expect("the test holds the receiver");
                if lost != Some(call) {
                    #[expect(
                        clippy::let_underscore_must_use,
                        reason = "a store thread that has gone drops the job, and its caller hears it failed"
                    )]
                    let _ = database.send(job);
                }
            }
        });
        (Self { jobs, thread }, heard)
    }

    /// A store whose thread has gone, as it would be after a panic there.
    #[cfg(test)]
    pub(crate) fn stopped() -> Self {
        let (jobs, _) = std::sync::mpsc::channel();
        Self {
            jobs,
            thread: std::thread::spawn(|| {}),
        }
    }
}

/// An Issue or a PR as quick open reads it and lists it. Quick open hands it to the window as it is, so a
/// field added here reaches the window unless it is skipped, as `updated_at` is; a test of quick open pins
/// what the window gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct TitledItem {
    pub number: u32,
    pub title: String,
    /// What quick open orders by, which the window doesn't show.
    #[serde(skip)]
    #[specta(skip)]
    pub updated_at: OffsetDateTime,
    #[serde(flatten)]
    pub kind: TitledKind,
}

/// Whether a `TitledItem` is an Issue or a PR, with its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TitledKind {
    Issue {
        /// Nothing for an Issue stored before Tsugiki kept states, until GitHub sends it again.
        state: Option<IssueState>,
    },
    PullRequest {
        state: PrState,
        is_draft: bool,
    },
}

/// Saves PR `facts` the user fetched, as `Store::save_unstored_pull_request` says.
fn save_unstored_in(
    connection: &mut Connection,
    repository: &RepositoryName,
    (facts, activity): (PullRequestFacts, ItemActivity),
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let synced = sync_state::read(&transaction, repository)?;
    let first = if synced.is_some_and(|state| facts.updated_at >= state.changes_since()) {
        FirstSave::MayHaveOpened
    } else {
        FirstSave::Loaded
    };
    let saving = (std::slice::from_ref(&facts), first);
    pull_requests::save_in(&transaction, repository, saving, &[activity])?;
    transaction.commit()
}

fn read_last_login(connection: &Connection) -> rusqlite::Result<Option<Login>> {
    connection
        .query_row("SELECT login FROM viewer WHERE id = 1", [], |row| {
            row.get("login")
        })
        .optional()
}

fn write_last_login(connection: &Connection, login: &Login) -> rusqlite::Result<()> {
    connection
        .execute(
            "INSERT INTO viewer (id, login) VALUES (1, :login)
             ON CONFLICT (id) DO UPDATE SET login = excluded.login
             WHERE login <> excluded.login",
            named_params! { ":login": login },
        )
        .map(drop)
}

// Add each read a screen waits on to `view_statements` in the tests, which check its plans (ADR 0017).
fn read_work_facts(
    connection: &mut Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<WorkFacts> {
    work_facts_in(&connection.transaction()?, repository)
}

fn work_facts_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
) -> rusqlite::Result<WorkFacts> {
    let viewer = read_last_login(transaction)?;
    let pull_requests = pull_requests::read_all(transaction, repository)?;
    Ok(WorkFacts {
        activity: activity::read_facts(transaction, repository, viewer.as_ref())?,
        reviews: patch_ids::read_facts(transaction, repository, &pull_requests)?,
        viewer,
        pull_requests,
        issues: issues::read_all(transaction, repository)?,
        filters: filters::read_all(transaction, repository)?,
    })
}

/// The Issues and the PRs that quick open searches, from one transaction (ADR 0018).
fn read_titled(
    connection: &mut Connection,
    repository: &RepositoryName,
) -> rusqlite::Result<Vec<TitledItem>> {
    let transaction = connection.transaction()?;
    let mut titled = issues::read_titled(&transaction, repository)?;
    titled.extend(pull_requests::read_titled(&transaction, repository)?);
    Ok(titled)
}

/// One Repository's facts as one read transaction sees them, for a view that works out what to read next
/// from what it has read, as the Inbox does (ADR 0018). Each read takes only the rows it names, so its cost
/// follows what it asks for and not what the store holds. `StoreReader::read_snapshot` hands one out.
pub struct Snapshot<'a> {
    transaction: Transaction<'a>,
    repository: RepositoryName,
}

impl Snapshot<'_> {
    /// The user signed in when Tsugiki last reached GitHub, if it ever has.
    pub fn viewer(&self) -> Result<Option<Login>, Error> {
        read_last_login(&self.transaction).map_err(saying(CANT_READ))
    }

    /// The Issues a PR that opened on them brought back to the Inbox (#823), closed or not.
    pub fn brought_back(&self) -> Result<BTreeSet<u32>, Error> {
        activity::brought_back(&self.transaction, &self.repository).map_err(saying(CANT_READ))
    }

    /// The Repository's "Show muted" (#825).
    pub fn show_muted(&self) -> Result<ShowMuted, Error> {
        activity::show_muted(&self.transaction, &self.repository).map_err(saying(CANT_READ))
    }

    /// The links of each PR with these numbers that the store holds, under `conventions`, in number order.
    pub fn pr_links(
        &self,
        numbers: &BTreeSet<u32>,
        conventions: Conventions,
    ) -> Result<Vec<PrLinks>, Error> {
        let numbers = Numbers::of(numbers.iter().copied());
        let (transaction, repository) = (&self.transaction, &self.repository);
        pr_links::read(transaction, repository, conventions, &numbers).map_err(saying(CANT_READ))
    }

    /// The PRs that fix one of `issues` under `conventions`.
    pub fn prs_fixing(
        &self,
        issues: &BTreeSet<u32>,
        conventions: Conventions,
    ) -> Result<BTreeSet<u32>, Error> {
        let issues = Numbers::of(issues.iter().copied());
        let (transaction, repository) = (&self.transaction, &self.repository);
        pr_links::fixing(transaction, repository, conventions, &issues).map_err(saying(CANT_READ))
    }

    /// The PRs that name one of `origins` as their Origin PR under `conventions`, and some that name it
    /// below another their links put first.
    pub fn prs_naming_origins(
        &self,
        origins: &BTreeSet<u32>,
        conventions: Conventions,
    ) -> Result<BTreeSet<u32>, Error> {
        let origins = Numbers::of(origins.iter().copied());
        let (transaction, repository) = (&self.transaction, &self.repository);
        let named = pr_links::naming_origins(transaction, repository, conventions, &origins);
        named.map_err(saying(CANT_READ))
    }

    /// Each PR whose last update `updates` takes, whatever its state, with that update, the latest first.
    pub fn pull_requests_updated(
        &self,
        updates: Updates,
    ) -> Result<Vec<(u32, OffsetDateTime)>, Error> {
        pr_links::updated(&self.transaction, &self.repository, updates).map_err(saying(CANT_READ))
    }

    /// The PRs with these numbers that the store holds, the latest update first.
    pub fn pull_requests_numbered(
        &self,
        numbers: &BTreeSet<u32>,
    ) -> Result<Vec<PullRequestFacts>, Error> {
        let numbers = Numbers::of(numbers.iter().copied());
        pull_requests::read(
            &self.transaction,
            &self.repository,
            Items::Numbered(&numbers),
        )
        .map_err(saying(CANT_READ))
    }

    /// The Issues with these numbers that the store holds, whatever their state, in number order.
    pub fn issues_numbered(&self, numbers: &BTreeSet<u32>) -> Result<Vec<IssueFacts>, Error> {
        let numbers = Numbers::of(numbers.iter().copied());
        issues::read(
            &self.transaction,
            &self.repository,
            Items::Numbered(&numbers),
        )
        .map_err(saying(CANT_READ))
    }

    /// Where each Issue with these numbers that the store holds stands, whatever its state, in number order.
    pub fn issue_places_numbered(&self, numbers: &BTreeSet<u32>) -> Result<Vec<IssuePlace>, Error> {
        let numbers = Numbers::of(numbers.iter().copied());
        self.issue_places(issues::PlaceScope::Numbered(&numbers))
    }

    /// Where each open Issue whose last update `updates` takes stands, the latest update first and then the
    /// highest number first.
    pub fn open_issue_places(&self, updates: Updates) -> Result<Vec<IssuePlace>, Error> {
        self.issue_places(issues::PlaceScope::Open(updates))
    }

    fn issue_places(&self, scope: issues::PlaceScope<'_>) -> Result<Vec<IssuePlace>, Error> {
        issues::read_places(&self.transaction, &self.repository, scope).map_err(saying(CANT_READ))
    }

    /// The activity of the Issues and PRs `numbers`, as `viewer` reads it.
    pub fn activity_of(
        &self,
        viewer: Option<&Login>,
        numbers: &BTreeSet<u32>,
    ) -> Result<ActivityFacts, Error> {
        let numbers = Numbers::of(numbers.iter().copied());
        let items = Items::Numbered(&numbers);
        activity::read_facts_of(&self.transaction, &self.repository, viewer, items)
            .map_err(saying(CANT_READ))
    }

    /// What the Stale review check of `pull_requests` reads.
    pub fn reviews_of(&self, pull_requests: &[PullRequestFacts]) -> Result<ReviewFacts, Error> {
        patch_ids::read_facts(&self.transaction, &self.repository, pull_requests)
            .map_err(saying(CANT_READ))
    }

    /// Each search of the Repository, with its last results.
    pub fn filter_results(&self) -> Result<BTreeMap<FilterQuery, FilterResults>, Error> {
        filters::read_all(&self.transaction, &self.repository).map_err(saying(CANT_READ))
    }

    /// What Issue `number` says, once Tsugiki has read it.
    pub fn issue_content(&self, number: u32) -> Result<Option<IssueContent>, Error> {
        issue_content::read(&self.transaction, &self.repository, number).map_err(saying(CANT_READ))
    }

    /// PR `number`'s Conversation, once Tsugiki has read it.
    pub fn pull_request_content(&self, number: u32) -> Result<Option<PullRequestContent>, Error> {
        let (transaction, repository) = (&self.transaction, &self.repository);
        pull_request_content::read(transaction, repository, number).map_err(saying(CANT_READ))
    }
}

/// Asks the read-only connection, from any thread. A question asked before the database opens waits for it.
#[derive(Clone)]
pub(crate) struct StoreReader {
    jobs: Weak<Sender<Job>>,
}

impl StoreReader {
    /// The facts behind the Workbench of `repository`.
    pub fn work_facts(&self, repository: &RepositoryName) -> Result<WorkFacts, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_work_facts(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// PR `number`'s facts, and its Conversation once Tsugiki has read it, or nothing if the store lacks the
    /// PR.
    pub fn pull_request(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<(PullRequestFacts, Option<PullRequestContent>)>, Error> {
        self.read_snapshot(repository, move |snapshot| {
            let held = snapshot.pull_requests_numbered(&BTreeSet::from([number]))?;
            let Some(pr) = held.into_iter().next() else {
                return Ok(None);
            };
            Ok(Some((pr, snapshot.pull_request_content(number)?)))
        })
    }

    /// Whether the store marks `repository` as one the token can't write to (#398).
    pub fn token_cant_write(&self, repository: &RepositoryName) -> Result<bool, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_only::marked(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// The number, title, state and last update of each Issue and PR of `repository`, in no order.
    pub fn titled_items(&self, repository: &RepositoryName) -> Result<Vec<TitledItem>, Error> {
        let repository = repository.clone();
        let read = self.call(move |connection| read_titled(connection, &repository))?;
        read.map_err(saying(CANT_READ))
    }

    /// What `read` answers from a snapshot of `repository`, which it reads in one transaction.
    pub fn read_snapshot<T: Send + 'static>(
        &self,
        repository: &RepositoryName,
        read: impl FnOnce(&Snapshot<'_>) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let repository = repository.clone();
        let job = move |connection: &mut Connection| {
            let transaction = connection.transaction().map_err(saying(CANT_READ))?;
            read(&Snapshot {
                transaction,
                repository,
            })
        };
        self.call(job)?
    }

    fn call<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> T + Send + 'static,
    ) -> Result<T, Error> {
        let stopped = || Error::new(ErrorKind::Internal, STOPPED);
        ask(&*self.jobs.upgrade().ok_or_else(stopped)?, work)
    }
}

/// The read-only connection before the database is ready for it. Dropping this answers what's waiting.
#[must_use = "dropping this tells every reader that the database stopped"]
pub(crate) struct ReaderToOpen {
    jobs: Arc<Sender<Job>>,
    queue: Receiver<Job>,
}

/// The read-only connection, on its thread. Only this keeps the thread going, so the app's readers can't keep
/// the file open once the session ends.
#[must_use = "dropping this leaves the connection open until the process ends"]
pub(crate) struct OpenReader {
    jobs: Arc<Sender<Job>>,
    thread: JoinHandle<()>,
}

/// A way to ask the read-only connection, and what opens it when `Store::open` has made the file current.
pub(crate) fn reader() -> (ReaderToOpen, StoreReader) {
    let (jobs, queue) = mpsc::channel::<Job>();
    let jobs = Arc::new(jobs);
    let reader = StoreReader {
        jobs: Arc::downgrade(&jobs),
    };
    (ReaderToOpen { jobs, queue }, reader)
}

impl ReaderToOpen {
    /// Opens `path` read-only, on a thread of its own.
    pub fn open(self, path: &Path) -> Result<OpenReader, Error> {
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(saying(CANT_OPEN))?;
        let queue = self.queue;
        let thread = thread::spawn(move || {
            for job in queue {
                job(&mut connection);
            }
        });
        Ok(OpenReader {
            jobs: self.jobs,
            thread,
        })
    }
}

impl OpenReader {
    /// Finishes the jobs it has and closes the connection. Close this before `Store`: the last connection
    /// to close removes the `-wal` and `-shm` files.
    pub fn close(self) {
        drop(self.jobs);
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a thread that panicked has closed the file too, and `call` has already said so"
        )]
        let _ = self.thread.join();
    }
}

/// Runs `work` on the thread that owns a connection, and waits for its answer.
fn ask<T: Send + 'static>(
    jobs: &Sender<Job>,
    work: impl FnOnce(&mut Connection) -> T + Send + 'static,
) -> Result<T, Error> {
    let (reply, answer) = mpsc::channel();
    let job = Box::new(move |connection: &mut Connection| {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "this fails only if the caller has gone, and it waits for this answer"
        )]
        let _ = reply.send(work(connection));
    });
    jobs.send(job)
        .ok()
        .and_then(|()| answer.recv().ok())
        .ok_or_else(|| Error::new(ErrorKind::Internal, STOPPED))
}

fn open_with(path: &Path, migrations: &[Migration]) -> Result<Store, Error> {
    fs::create_dir_all(path.with_file_name("")).map_err(saying(CANT_OPEN))?;
    let lock = lock(path)?;
    file::check(path, migrate::latest(migrations))?;

    let mut connection = Connection::open(path).map_err(saying(CANT_OPEN))?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(saying(CANT_OPEN))?;
    // The bundled SQLite enforces foreign keys from the start, but another build might not.
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(saying(CANT_OPEN))?;
    migrate::run(&mut connection, path, migrations)?;
    // Only a file at the newest schema has the tables this version's code reads. Stale links stay as they
    // are: the session refills them, and a refill that fails leaves the store open (#950). Links a newer
    // Tsugiki worked out stay as they are too, and the store with them (#971).
    if migrate::latest(migrations) == migrate::latest(MIGRATIONS) {
        if pr_links::written_by_newer(&connection).map_err(saying(CANT_OPEN))? {
            return Err(Error::new(ErrorKind::Internal, NEWER));
        }
        pr_links::keep_set_if_empty(&mut connection).map_err(saying(CANT_UPDATE))?;
    }
    // A crash, a force-kill or a quit during a slow request skips closing the store, which would leave a new
    // version in the `-wal` file, where `file::check` can't see it. If another connection blocks this, SQLite
    // says so in the row and not as an error: `migrate` still catches the version, and the next start tries
    // again.
    connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(saying(CANT_UPDATE))?;

    let (jobs, queue) = mpsc::channel::<Job>();
    let thread = thread::spawn(move || {
        for job in queue {
            job(&mut connection);
        }
        drop(connection);
        drop(lock);
    });
    Ok(Store { jobs, thread })
}

/// How long a start waits for the lock before it says another Tsugiki has it. Another Tsugiki holds it for as
/// long as it runs. A program that another thread is starting holds a copy of the lock file for a moment,
/// until the system closes the files the program doesn't inherit. On macOS and Linux the lock belongs to the
/// open file and lasts while any copy is open, so a Store that has just closed can still look locked then
/// (#198). Windows starts a program with only the files it inherits.
const LOCK_PATIENCE: Duration = Duration::from_secs(1);

/// How long a start sleeps between two tries at the lock.
const LOCK_RETRY: Duration = Duration::from_millis(20);

/// Takes the lock beside the database at `path`, which one Tsugiki at a time holds while it writes (ADR
/// 0018). It belongs to the folder, so an installed Tsugiki, a development build and a portable copy each
/// hold their own. The system lets go of it when the file closes or the process ends, so a crash can't leave
/// the next start locked out. The empty file stays. A database in memory belongs to its one connection, so it
/// needs none.
fn lock(path: &Path) -> Result<Option<File>, Error> {
    if path == Path::new(":memory:") {
        return Ok(None);
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(migrate::with_suffix(path, ".lock"))
        .map_err(saying(CANT_OPEN))?;
    let deadline = Instant::now() + LOCK_PATIENCE;
    loop {
        match file.try_lock() {
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(LOCK_RETRY),
            Err(TryLockError::WouldBlock) => return Err(Error::new(ErrorKind::Internal, IN_USE)),
            locked => return locked.map(|()| Some(file)).map_err(saying(CANT_OPEN)),
        }
    }
}

/// Saves `state` as `repository`'s sync state, which leaves its first sync no place to come back to.
fn save_sync_state_in(
    transaction: &Transaction<'_>,
    repository: &RepositoryName,
    state: &SyncState,
) -> rusqlite::Result<()> {
    sync_state::save(transaction, repository, state)?;
    sync_state::forget_place(transaction, repository)?;
    activity::remember_first_sync(transaction, repository, state.started_at)
}

/// Turns a failure into words a user can read (`Error`). SQLite's or the OS's reason goes to the log.
fn saying<E: Display>(message: &'static str) -> impl FnOnce(E) -> Error {
    move |cause| {
        log::error!("{message}: {cause}");
        Error::new(ErrorKind::Internal, message)
    }
}

#[cfg(test)]
pub(super) mod testing;

#[cfg(test)]
mod tests;
