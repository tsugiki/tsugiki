//! The queue of syncs: which runs next, when it can run and when it gives up (ADR 0015).

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use super::issues::fetch_issue_batch;
use super::{Link, Step, UserWorkSync};
use crate::error::{Error, ErrorKind};
use crate::github::GitHub;
use crate::github::in_flight::Asking;
use crate::latest::LatestReader;
use crate::repository::RepositoryName;
use crate::repository_settings::RepositorySettings;
use crate::settings::SettingsView;
use crate::status::{Change, LastSync, OpenSync, SyncActivity, SyncPart};
use crate::store::Store;

/// A sync's step can fail this many times in a row for any reason but the network or the rate limit, with
/// one try at each refresh. GitHub's own outages give the same errors as a Repository that has gone, so this
/// lets one pass before the sync leaves the queue. Asking for that Repository again, or restarting, tries
/// again. The timer also tries a Repository that has finished a sync, since a refresh costs a few points. A
/// first sync costs thousands, so it waits for the user.
const TRIES: u8 = 10;

/// What a step of the queue did that the rest of the app can see.
#[derive(Debug, PartialEq, Eq)]
pub enum Progress {
    /// Stored facts changed.
    Changed(Change),
    /// The open Repository's sync finished.
    Finished(LastSync),
    /// What the Repository's page in Settings shows of its syncs changed, and nothing its other screens show:
    /// a sync saved a new count of its open items, or the sync of a Repository not open finished (#534).
    Recorded(RepositoryName),
}

/// A step that failed: the problem, for the status bar, and the work it holds back.
#[derive(Debug)]
pub struct StepFailure {
    /// It names the Repository when that isn't the open one, unless it speaks for all of GitHub.
    pub problem: Error,
    turn: Turn,
}

/// Why every step waits, and until when.
#[derive(Clone, Copy)]
enum Hold {
    /// Until the next look at GitHub after a failure. The user can ask sooner.
    Retry(Instant),
    /// Until GitHub's points come back, which asking sooner would only waste. The status bar shows the time.
    Points(Instant, OffsetDateTime),
}

impl Hold {
    fn until(self) -> Instant {
        match self {
            Self::Retry(at) | Self::Points(at, _) => at,
        }
    }
}

/// A sync in the queue, with what its own failures have done to it.
struct Queued {
    sync: UserWorkSync,
    /// The time its next step can run, after a failure that spoke for its Repository alone.
    held_until: Option<Instant>,
    /// Its failures in a row that count towards [`TRIES`].
    failures: u8,
}

impl Queued {
    fn new(sync: UserWorkSync) -> Self {
        Self {
            sync,
            held_until: None,
            failures: 0,
        }
    }

    fn is_ready(&self, now: Instant) -> bool {
        self.held_until.is_none_or(|at| at <= now)
    }
}

/// The read of the whole tree of an Issue the user opened.
struct OpenedTree {
    number: u32,
    sync: UserWorkSync,
    failures: u8,
}

/// The fetch of the Issues of a Repository saved before Tsugiki kept states (#231). It runs once a launch,
/// after the Repository's first sync finishes, in the steps no sync is ready for.
struct StateFill {
    repository: RepositoryName,
    /// The Issues still to fetch, lowest number first. Nothing, until the store says which they are.
    numbers: Option<Vec<u32>>,
}

/// The work that takes a step.
#[derive(Debug)]
enum Turn {
    /// The first of the opened Issues' trees.
    Tree,
    /// The first of the fetches of Issues with no state.
    Fill,
    /// The queued sync of this Repository.
    Sync(RepositoryName),
}

/// What a step that didn't finish its sync saved.
enum Stepped {
    Nothing,
    /// Stored facts, with the Issues and PRs they touched.
    Facts(Vec<u32>),
    /// A new count of the open items, and nothing else.
    Count,
}

/// The step as [`Stepped`], or nothing for one that finished its sync.
fn unfinished(step: Step) -> Option<Stepped> {
    Some(match step {
        Step::Finished => return None,
        Step::Moved | Step::Searched => Stepped::Nothing,
        Step::Saved { touched } => Stepped::Facts(touched),
        Step::Recounted => Stepped::Count,
    })
}

/// The syncs still to run, one step at a time. Repositories take turns, so a big first sync doesn't keep
/// the others waiting (ADR 0015). The open Repository has every other step, because the window shows it,
/// and the others take theirs in the order they were asked for, the latest first.
pub struct Syncs {
    queue: VecDeque<Queued>,
    /// The trees of Issues the user opened with no first sync's queue to put them in. Each goes ahead of
    /// `queue` for its few requests, and leaves no trace of a sync behind it.
    trees: VecDeque<OpenedTree>,
    /// The settings owner's view, which each step reads its Repository's settings from.
    settings: LatestReader<SettingsView>,
    /// The Repositories opened since launch, whose lists the window shows, so it hears of a hand edit to
    /// their settings.
    repositories_opened: Vec<RepositoryName>,
    /// The Issues of `open` the user has opened since it opened. Each reads its tree once, because the
    /// session reads an Issue on screen again about once a minute.
    opened: BTreeSet<u32>,
    /// A wait for all of GitHub, which holds back every step.
    not_before: Option<Hold>,
    /// The open Repository's sync took the last step, so another Repository's takes the next.
    open_went_last: bool,
    /// The Repository the window has open, which alone keeps refreshing.
    open: Option<RepositoryName>,
    /// The fetches of Issues with no state still to run, each behind the syncs that are ready.
    fills: VecDeque<StateFill>,
    /// The Repositories whose fetch of Issues with no state has been queued since launch. An Issue GitHub no
    /// longer has keeps no state, so each launch asks for it once.
    fills_queued: Vec<RepositoryName>,
}

impl Syncs {
    /// No syncs yet, which read each Repository's settings from the settings owner's `settings`.
    pub fn new(settings: LatestReader<SettingsView>) -> Self {
        Self {
            queue: VecDeque::new(),
            trees: VecDeque::new(),
            settings,
            repositories_opened: Vec::new(),
            opened: BTreeSet::new(),
            not_before: None,
            open_went_last: false,
            open: None,
            fills: VecDeque::new(),
            fills_queued: Vec::new(),
        }
    }

    /// Opens `repository` and gives its sync the next step, carrying on from where it got if one is already
    /// queued. A new sync searches the Filters the settings owner's view lists now.
    pub fn add(&mut self, repository: RepositoryName) {
        if !self.is_open(&repository) {
            self.opened.clear();
            self.trees.clear();
        }
        if !self.was_opened(&repository) {
            self.repositories_opened.push(repository.clone());
        }
        let queued = self.take(&repository);
        let sync = match queued {
            Some(queued) => queued.sync,
            None => UserWorkSync::new(&self.settings_of(&repository)),
        };
        self.queue.push_front(Queued::new(sync));
        self.open = Some(repository);
        self.open_went_last = false;
    }

    /// Whether `repository` was opened since launch, so the window lists its items.
    pub fn was_opened(&self, repository: &RepositoryName) -> bool {
        (self.repositories_opened.iter()).any(|opened| opened.is_same(repository))
    }

    /// Queues the timer's refresh of the open Repository, unless a sync of it is under way or waiting. A
    /// store that can't answer queues nothing, because the sync couldn't save, and the status says why.
    pub fn refresh_open(&mut self, store: &Store) {
        let Some(open) = self.open_settings() else {
            return;
        };
        let has_synced = matches!(store.sync_state(&open.name), Ok(Some(_)));
        if has_synced && self.position(&open.name).is_none() {
            let sync = UserWorkSync::new(&open);
            self.queue.push_back(Queued::new(sync));
        }
    }

    /// Answers the user's click: the open Repository takes the next step, and stops waiting for the next
    /// look at GitHub. A wait for GitHub's points stays.
    pub fn refresh_now(&mut self) {
        self.put_open_first();
        if let Some(Hold::Retry(_)) = self.not_before {
            self.not_before = None;
        }
    }

    /// Answers the user's click while GitHub can't be reached: the open Repository takes the next step once
    /// the wait for the next look at GitHub ends.
    pub fn put_open_first(&mut self) {
        if let Some(open) = self.open.clone() {
            self.add(open);
        }
    }

    /// The work going on, for the status bar. The other Repositories go by name, so their turns don't
    /// change it.
    pub fn activity(&self) -> Option<SyncActivity> {
        let open = match self.trees.front() {
            Some(tree) => Some((&tree.sync, SyncPart::Trees)),
            None => self
                .queue
                .iter()
                .find(|queued| self.is_open(&queued.sync.repository))
                .map(|queued| (&queued.sync, queued.sync.part())),
        };
        if open.is_none() && self.queue.is_empty() {
            return None;
        }
        let mut elsewhere: Vec<RepositoryName> = self
            .queue
            .iter()
            .map(|queued| &queued.sync.repository)
            .filter(|repository| !self.is_open(repository))
            .cloned()
            .collect();
        elsewhere.sort_by_key(|repository| repository.to_string().to_ascii_lowercase());
        let paused_until = match self.not_before {
            Some(Hold::Points(_, until)) => Some(until),
            Some(Hold::Retry(_)) | None => None,
        };
        Some(SyncActivity {
            open: open.map(|(sync, part)| OpenSync {
                repository: sync.repository.clone(),
                part,
                pages: sync.first_sync().map_or(0, |first_sync| first_sync.pages),
                load: sync.load_progress(),
            }),
            paused_until,
            elsewhere,
        })
    }

    /// Tells the syncs the user opened Issue `number` of the open Repository. A first sync's queue moves its
    /// tree up. With no queue the tree gets its own read, because the PRs older than the window aren't in
    /// yet.
    pub fn open_issue(&mut self, repository: &RepositoryName, number: u32) {
        if !self.is_open(repository) || !self.opened.insert(number) {
            return;
        }
        let queued = self
            .queued_mut(repository)
            .is_some_and(|queued| queued.sync.open_issue(number));
        if queued {
            return;
        }
        if let Some(open) = self.open_settings() {
            self.trees.push_back(OpenedTree {
                number,
                sync: UserWorkSync::tree_of(&open, number),
                failures: 0,
            });
        }
    }

    /// The settings of `repository` in the settings owner's view now, or the defaults for one they don't
    /// list, as a hand edit can take a Repository out while its sync runs.
    fn settings_of(&self, repository: &RepositoryName) -> RepositorySettings {
        (self.settings).read(|view| view.repository(repository.clone()))
    }

    fn open_settings(&self) -> Option<RepositorySettings> {
        Some(self.settings_of(self.open.as_ref()?))
    }

    fn is_open(&self, repository: &RepositoryName) -> bool {
        self.open
            .as_ref()
            .is_some_and(|open| open.is_same(repository))
    }

    fn position(&self, repository: &RepositoryName) -> Option<usize> {
        self.queue
            .iter()
            .position(|queued| queued.sync.repository.is_same(repository))
    }

    fn queued_mut(&mut self, repository: &RepositoryName) -> Option<&mut Queued> {
        self.queue
            .iter_mut()
            .find(|queued| queued.sync.repository.is_same(repository))
    }

    /// Takes `repository`'s sync out of the queue.
    fn take(&mut self, repository: &RepositoryName) -> Option<Queued> {
        self.queue.remove(self.position(repository)?)
    }

    /// The place of the sync that takes the next step. Of the syncs that aren't held back, the open
    /// Repository's and the first of the others take turns. With every sync held back, the one free soonest
    /// goes.
    fn next_sync(&self) -> Option<usize> {
        let now = Instant::now();
        let ready = |open: bool| {
            self.queue.iter().find(|queued| {
                self.is_open(&queued.sync.repository) == open && queued.is_ready(now)
            })
        };
        let next = match (ready(true), ready(false)) {
            (Some(_), Some(other)) if self.open_went_last => other,
            (Some(ready), _) | (None, Some(ready)) => ready,
            (None, None) => self.queue.iter().min_by_key(|queued| queued.held_until)?,
        };
        self.position(&next.sync.repository)
    }

    /// The time the next step can run, if there is one.
    pub fn ready_at(&self) -> Option<Instant> {
        let free = if self.trees.is_empty() && self.fills.is_empty() {
            self.queue
                .iter()
                .map(|queued| queued.held_until.unwrap_or_else(Instant::now))
                .min()?
        } else {
            Instant::now()
        };
        Some(self.not_before.map_or(free, |hold| hold.until().max(free)))
    }

    /// Holds every step back until `at`.
    pub fn wait_until(&mut self, at: Instant) {
        self.not_before = Some(Hold::Retry(at));
    }

    /// Holds back the work whose step failed: all of it if the problem speaks for all of GitHub, or else that
    /// sync alone, so the other Repositories carry on. It waits until `next_refresh`, or for the rate limit
    /// until GitHub's points come back. It also takes a sync that keeps failing out of the queue.
    pub fn after_failure(&mut self, failure: &StepFailure, next_refresh: Instant) {
        let problem = &failure.problem;
        let hold = hold_after(problem, next_refresh);
        if counts_toward_tries(problem.kind) {
            self.count_failure(&failure.turn, problem);
        }
        match &failure.turn {
            Turn::Sync(repository) if !problem.kind.speaks_for_all_of_github() => {
                if let Some(queued) = self.queued_mut(repository) {
                    queued.held_until = Some(hold.until());
                }
            }
            // A tree's or a fill's own trouble ends it in the step, so they fail only for all of GitHub.
            Turn::Sync(_) | Turn::Tree | Turn::Fill => self.not_before = Some(hold),
        }
    }

    /// Holds every step back after fetching an opened Issue failed, if `problem` speaks for all of GitHub. An
    /// Issue's own trouble holds back no sync.
    pub fn after_fetch_failure(&mut self, problem: &Error, next_refresh: Instant) {
        if problem.kind.speaks_for_all_of_github() {
            self.not_before = Some(hold_after(problem, next_refresh));
        }
    }

    fn count_failure(&mut self, turn: &Turn, problem: &Error) {
        match turn {
            // A fill costs a request a try, and waits for the next refresh like the rest.
            Turn::Fill => {}
            Turn::Tree => {
                let gave_up = self.trees.front_mut().is_some_and(|tree| {
                    tree.failures += 1;
                    tree.failures >= TRIES
                });
                if gave_up && let Some(tree) = self.trees.pop_front() {
                    self.give_up_on(&tree, problem);
                }
            }
            Turn::Sync(repository) => {
                let Some(queued) = self.queued_mut(repository) else {
                    return;
                };
                queued.failures += 1;
                if queued.failures >= TRIES
                    && let Some(queued) = self.take(repository)
                {
                    log::error!(
                        "Tsugiki gave up syncing {}: {}",
                        queued.sync.repository,
                        problem.message
                    );
                }
            }
        }
    }

    /// Forgets that `tree`'s Issue was opened, so opening it asks again, as does the session's read of an
    /// Issue on screen about once a minute.
    fn give_up_on(&mut self, tree: &OpenedTree, problem: &Error) {
        let number = tree.number;
        self.opened.retain(|opened| *opened != number);
        log::warn!(
            "Tsugiki couldn't read the tree of Issue #{number}: {}",
            problem.message
        );
    }

    /// Runs one step of the work whose turn it is. Answers with what changed for the rest of the app, if
    /// anything did.
    pub fn step(
        &mut self,
        github: &GitHub,
        store: &Store,
    ) -> Result<Option<Progress>, StepFailure> {
        // A tree goes first, and a fill only when no sync is ready.
        if let Some(tree) = self.trees.pop_front() {
            return self.step_tree(tree, github, store);
        }
        let now = Instant::now();
        if !self.queue.iter().any(|queued| queued.is_ready(now))
            && let Some(fill) = self.fills.pop_front()
        {
            return self.step_fill(fill, github, store);
        }
        let Some(taken) = self
            .next_sync()
            .and_then(|at| Some((at, self.queue.remove(at)?)))
        else {
            return Ok(None);
        };
        self.step_sync(taken, github, store)
    }

    /// Runs a step of `tree`, which goes back to the front of the trees until it finishes.
    fn step_tree(
        &mut self,
        mut tree: OpenedTree,
        github: &GitHub,
        store: &Store,
    ) -> Result<Option<Progress>, StepFailure> {
        self.not_before = None;
        let settings = self.settings_of(&tree.sync.repository);
        let step = match tree.sync.step(github, store, &settings) {
            Ok(step) => step,
            // One Issue's trouble mustn't stand in the way of the syncs behind it.
            Err(problem) if !problem.kind.speaks_for_all_of_github() => {
                self.give_up_on(&tree, &problem);
                return Ok(None);
            }
            Err(problem) => {
                self.trees.push_front(tree);
                return Err(StepFailure {
                    problem,
                    turn: Turn::Tree,
                });
            }
        };
        tree.failures = 0;
        let repository = tree.sync.repository.clone();
        // A tree isn't a whole sync, so it doesn't move the time of the last one.
        let Some(stepped) = unfinished(step) else {
            return Ok(None);
        };
        self.trees.push_front(tree);
        Ok(self.after_step(repository, stepped, github))
    }

    /// Runs a step of the sync `queued`, taken from place `at`, which then goes to the back of the queue,
    /// behind the others'. A step that fails puts it back in its place.
    fn step_sync(
        &mut self,
        (at, mut queued): (usize, Queued),
        github: &GitHub,
        store: &Store,
    ) -> Result<Option<Progress>, StepFailure> {
        self.not_before = None;
        let is_open = self.is_open(&queued.sync.repository);
        queued.held_until = None;
        let settings = self.settings_of(&queued.sync.repository);
        let stepped = queued.sync.step(github, store, &settings);
        let stepped = match stepped.map(unfinished) {
            Ok(Some(stepped)) => stepped,
            Ok(None) => match queued.sync.finish(store) {
                Ok(finished) => {
                    self.queue_fill(&finished.repository);
                    self.open_went_last = is_open;
                    return Ok(Some(if is_open {
                        Progress::Finished(finished)
                    } else {
                        Progress::Recorded(finished.repository)
                    }));
                }
                Err(problem) => return Err(self.put_back(at, queued, problem)),
            },
            Err(problem) => return Err(self.put_back(at, queued, problem)),
        };
        self.open_went_last = is_open;
        queued.failures = 0;
        let repository = queued.sync.repository.clone();
        self.queue.push_back(queued);
        Ok(self.after_step(repository, stepped, github))
    }

    /// Puts back a sync whose step failed, at `at`. The status bar shows the problem beside the open
    /// Repository, so it names another Repository's, unless the problem speaks for all of GitHub.
    fn put_back(&mut self, at: usize, queued: Queued, problem: Error) -> StepFailure {
        let repository = queued.sync.repository.clone();
        self.queue.insert(at, queued);
        let problem = if self.is_open(&repository) || problem.kind.speaks_for_all_of_github() {
            problem
        } else {
            Error {
                kind: problem.kind,
                message: format!("{repository}: {}", problem.message),
            }
        };
        StepFailure {
            problem,
            turn: Turn::Sync(repository),
        }
    }

    /// Queues the fetch of `repository`'s Issues with no state, unless one was queued since launch.
    fn queue_fill(&mut self, repository: &RepositoryName) {
        if self
            .fills_queued
            .iter()
            .any(|queued| queued.is_same(repository))
        {
            return;
        }
        self.fills_queued.push(repository.clone());
        self.fills.push_back(StateFill {
            repository: repository.clone(),
            numbers: None,
        });
    }

    /// Runs a step of `fill`, which goes back to the front of the fills until it has no Issue left.
    fn step_fill(
        &mut self,
        mut fill: StateFill,
        github: &GitHub,
        store: &Store,
    ) -> Result<Option<Progress>, StepFailure> {
        self.not_before = None;
        let stepped = match fill_step(&mut fill, github, store) {
            Ok(stepped) => stepped,
            // The Issues' own trouble mustn't stand in the way of the syncs, so it waits for the next launch.
            Err(problem) if !problem.kind.speaks_for_all_of_github() => {
                log::warn!(
                    "Tsugiki couldn't fetch the states of {}'s Issues: {}",
                    fill.repository,
                    problem.message
                );
                return Ok(None);
            }
            Err(problem) => {
                self.fills.push_front(fill);
                return Err(StepFailure {
                    problem,
                    turn: Turn::Fill,
                });
            }
        };
        let repository = fill.repository.clone();
        if !fill.numbers.as_ref().is_some_and(Vec::is_empty) {
            self.fills.push_front(fill);
        }
        Ok(self.after_step(repository, stepped, github))
    }

    /// Passes on what a step saved, and holds every step back while the rate limit's gate holds, until
    /// GitHub's points come back.
    fn after_step(
        &mut self,
        repository: RepositoryName,
        stepped: Stepped,
        github: &GitHub,
    ) -> Option<Progress> {
        self.not_before = github.points_back().map(instant_of).map(points_hold);
        match stepped {
            Stepped::Nothing => None,
            Stepped::Facts(items) => Some(Progress::Changed(Change { repository, items })),
            Stepped::Count => Some(Progress::Recorded(repository)),
        }
    }
}

/// One step of `fill`: the store's list of the Issues with no state, or GitHub's answer for the next batch.
fn fill_step(fill: &mut StateFill, github: &GitHub, store: &Store) -> Result<Stepped, Error> {
    let Some(numbers) = &mut fill.numbers else {
        fill.numbers = Some(store.issues_without_state(&fill.repository)?);
        return Ok(Stepped::Nothing);
    };
    let link = Link {
        github,
        store,
        repository: &fill.repository,
        filters: &[],
        window_since: None,
    };
    let asked = Asking::Sync {
        repository: fill.repository.clone(),
    };
    let saved = github.asking(asked, || fetch_issue_batch(&link, numbers))?;
    Ok(if saved.touched.is_empty() {
        Stepped::Nothing
    } else {
        Stepped::Facts(saved.touched)
    })
}

/// The wait after `problem`: for the rate limit, until GitHub's points come back, as its gate says, and
/// else, or for a reset that has passed, until `next_refresh`.
fn hold_after(problem: &Error, next_refresh: Instant) -> Hold {
    match problem.kind {
        ErrorKind::RateLimited { resets_at } => {
            let back = resets_at
                .and_then(|at| OffsetDateTime::from_unix_timestamp(i64::from(at)).ok())
                .filter(|at| *at > OffsetDateTime::now_utc());
            points_hold(back.map_or(next_refresh, instant_of))
        }
        ErrorKind::Offline
        | ErrorKind::CertificateDates
        | ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::NotFound
        | ErrorKind::Conflict
        | ErrorKind::QueryRefused
        | ErrorKind::Internal => Hold::Retry(next_refresh),
    }
}

/// A failure of this kind counts towards [`TRIES`]. The network and the rate limit come back by themselves,
/// and fixing the clock is up to the user, and the next try after it goes through.
fn counts_toward_tries(kind: ErrorKind) -> bool {
    match kind {
        ErrorKind::RateLimited { .. } | ErrorKind::Offline | ErrorKind::CertificateDates => false,
        ErrorKind::Unauthorized
        | ErrorKind::WriteRefused
        | ErrorKind::NotFound
        | ErrorKind::Conflict
        | ErrorKind::QueryRefused
        | ErrorKind::Internal => true,
    }
}

/// A wait for GitHub's points until `at`.
fn points_hold(at: Instant) -> Hold {
    let wait = at.saturating_duration_since(Instant::now());
    Hold::Points(at, OffsetDateTime::now_utc() + wait)
}

/// The instant this computer's clock reaches `at`, which the gate holds no more than an hour away.
fn instant_of(at: OffsetDateTime) -> Instant {
    Instant::now() + Duration::try_from(at - OffsetDateTime::now_utc()).unwrap_or(Duration::ZERO)
}

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;
