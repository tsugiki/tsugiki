//! Keeping each mirror in shape (#174). The refs of PRs the user's work no longer reaches go, though their
//! reviewed refs stay (#382), git's packs get their upkeep on a schedule, and a mirror git says is broken is
//! cloned again. The worker runs it all itself, with `maintenance.auto=false` in each mirror, and never
//! `git maintenance start`, which writes to the user's global config and installs a job in the OS.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fs, io, mem};

use super::{Behind, Job, Run, Turn, Worker};
use crate::repository::RepositoryName;

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// The file beside a mirror, in its Repository's folder, that says when it last had its upkeep.
const UPKEEP_FILE: &str = "upkeep.toml";

/// The folder a broken mirror waits in until its new clone deletes it.
const BROKEN: &str = "mirror.broken";

/// The intervals between each mirror's upkeep.
#[derive(Debug, Clone, Copy)]
pub(super) struct Schedule {
    /// The time between runs of `git maintenance run`'s incremental tasks.
    pub(super) maintain: Duration,
    /// The time between runs of `git gc`, which repacks everything, so it stands in for that day's
    /// maintenance.
    pub(super) collect: Duration,
}

impl Schedule {
    /// The schedule #13 decided: maintenance each day, and gc each week.
    pub(super) const DAILY_AND_WEEKLY: Self = Self {
        maintain: DAY,
        collect: DAY.saturating_mul(7),
    };
}

/// One of the two kinds of upkeep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Task {
    Maintain,
    Collect,
}

impl Task {
    /// git's arguments for this task, after `-C <mirror>`.
    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Maintain => &[
                "maintenance",
                "run",
                "--quiet",
                "--task=commit-graph",
                "--task=loose-objects",
                "--task=incremental-repack",
            ],
            Self::Collect => &["gc", "--quiet"],
        }
    }
}

/// The times a mirror last had each kind of upkeep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Upkept {
    maintained: SystemTime,
    collected: SystemTime,
}

impl Upkept {
    /// A mirror whose upkeep is all as good as new at `time`, as a fresh clone is.
    fn at(time: SystemTime) -> Self {
        Self {
            maintained: time,
            collected: time,
        }
    }

    /// The times the file beside `mirror` gives. A time Tsugiki can't read counts as 1970, as for a mirror
    /// cloned before Tsugiki kept it, so its upkeep is due at once.
    fn read(mirror: &Path) -> Self {
        let text = fs::read_to_string(mirror.with_file_name(UPKEEP_FILE)).unwrap_or_default();
        let document = text.parse::<toml_edit::DocumentMut>().unwrap_or_default();
        let time = |key| {
            let seconds = document.get(key).and_then(toml_edit::Item::as_integer);
            let seconds = seconds.and_then(|seconds| u64::try_from(seconds).ok());
            seconds
                .and_then(|seconds| UNIX_EPOCH.checked_add(Duration::from_secs(seconds)))
                .unwrap_or(UNIX_EPOCH)
        };
        Self {
            maintained: time("maintained"),
            collected: time("collected"),
        }
    }

    /// Writes this beside `mirror`, as seconds since 1970.
    fn write(self, mirror: &Path) -> io::Result<()> {
        let seconds = |time: SystemTime| {
            time.duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        };
        let text = format!(
            "maintained = {}\ncollected = {}\n",
            seconds(self.maintained),
            seconds(self.collected)
        );
        fs::write(mirror.with_file_name(UPKEEP_FILE), text)
    }

    /// The task due at `now`, if one is.
    fn due(self, schedule: Schedule, now: SystemTime) -> Option<Task> {
        if self.next_collect(schedule) <= now {
            Some(Task::Collect)
        } else if self.next_maintain(schedule) <= now {
            Some(Task::Maintain)
        } else {
            None
        }
    }

    /// The time the next task falls due.
    fn next(self, schedule: Schedule) -> SystemTime {
        self.next_collect(schedule)
            .min(self.next_maintain(schedule))
    }

    fn next_collect(self, schedule: Schedule) -> SystemTime {
        self.collected
            .checked_add(schedule.collect)
            .unwrap_or(self.collected)
    }

    fn next_maintain(self, schedule: Schedule) -> SystemTime {
        self.maintained
            .checked_add(schedule.maintain)
            .unwrap_or(self.maintained)
    }

    /// This, with `task` done at `time`.
    fn after(self, task: Task, time: SystemTime) -> Self {
        match task {
            Task::Maintain => Self {
                maintained: time,
                ..self
            },
            Task::Collect => Self::at(time),
        }
    }
}

/// One mirror's upkeep: first deleting the refs of the PRs in `pruning`, if there are any, then `task`.
struct Plan {
    repository: RepositoryName,
    mirror: PathBuf,
    pruning: Vec<u32>,
    task: Task,
}

/// The git of a mirror's upkeep under way.
pub(super) struct Upkeep {
    plan: Plan,
    pub(super) git: Box<dyn Run>,
}

/// Whether git's words say the mirror itself is broken. The network, GitHub and the disk fail in others.
/// Words git quotes, such as a URL or a path, don't count: a Repository may have a sign in its name.
pub(super) fn says_broken(words: &str) -> bool {
    const SIGNS: [&str; 8] = [
        "corrupt",
        "bad object",
        "bad tree object",
        "missing blob",
        "missing tree",
        "missing commit",
        "missing object",
        "not a git repository",
    ];
    let unquoted: String = words.split('\'').step_by(2).collect();
    let unquoted = unquoted.to_ascii_lowercase();
    SIGNS.iter().any(|sign| unquoted.contains(sign))
}

/// Deletes the broken mirror beside `repository`'s `mirror`, if there is one, and says whether it did.
pub(super) fn delete_broken(repository: &RepositoryName, mirror: &Path) -> bool {
    let broken = mirror.with_file_name(BROKEN);
    if !broken.exists() {
        return false;
    }
    match fs::remove_dir_all(&broken) {
        Ok(()) => {
            log::info!("Tsugiki deleted {repository}'s broken mirror");
            true
        }
        Err(cause) => {
            log::warn!("Tsugiki can't delete {repository}'s broken mirror: {cause}");
            false
        }
    }
}

/// Writes `upkept` beside `repository`'s `mirror`, or leaves a trace if it can't. The worker goes by its
/// own copy until the next launch.
fn write_upkept(repository: &RepositoryName, mirror: &Path, upkept: Upkept) {
    if let Err(cause) = upkept.write(mirror) {
        log::warn!("Tsugiki can't note the upkeep of {repository}'s mirror: {cause}");
    }
}

/// Deletes the folder at `path`, which may not be there.
pub(super) fn remove_if_there(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

impl Worker {
    /// Notes when `repository`'s mirror last had its upkeep, the first time the worker hears of it.
    pub(super) fn note_for_upkeep(&mut self, repository: &RepositoryName) {
        let (Some(_), Ok(folders)) = (self.remotes.upkeep, &self.folders) else {
            return;
        };
        if self.upkept.iter().any(|(one, _)| one.is_same(repository)) {
            return;
        }
        let upkept = Upkept::read(&folders.mirror(repository));
        self.upkept.push((repository.clone(), upkept));
    }

    /// The time the first mirror's upkeep falls due.
    pub(super) fn upkeep_due(&self) -> Option<Instant> {
        let schedule = self.remotes.upkeep?;
        let folders = self.folders.as_ref().ok()?;
        let next = (self.upkept.iter())
            .filter(|(repository, _)| folders.mirror(repository).exists())
            .map(|(_, upkept)| upkept.next(schedule))
            .min()?;
        let wait = next.duration_since(SystemTime::now()).unwrap_or_default();
        Instant::now().checked_add(wait)
    }

    /// Starts the upkeep of the first mirror that is due, if one is. It reads the mirror's PR refs first,
    /// to delete those of PRs the user's work no longer reaches.
    pub(super) fn start_upkeep(&mut self) {
        let (Some(schedule), Ok(folders)) = (self.remotes.upkeep, &self.folders) else {
            return;
        };
        let now = SystemTime::now();
        let due = self.upkept.iter().find_map(|(repository, upkept)| {
            let task = upkept.due(schedule, now)?;
            let mirror = folders.mirror(repository);
            mirror.exists().then(|| (repository.clone(), mirror, task))
        });
        let Some((repository, mirror, task)) = due else {
            return;
        };
        let pruning = self.stale_refs(&repository, &mirror);
        self.start_upkeep_git(Plan {
            repository,
            mirror,
            pruning,
            task,
        });
    }

    /// The PRs whose refs are in `mirror` though the last hand-over didn't reach them and the user hasn't
    /// opened them since launch. None before the first hand-over, which says what the user's work reaches.
    fn stale_refs(&self, repository: &RepositoryName, mirror: &Path) -> Vec<u32> {
        let known = self
            .heads
            .iter()
            .find(|heads| heads.repository.is_same(repository));
        let Some((handed, opened)) =
            known.and_then(|heads| Some((heads.handed.as_ref()?, &heads.viewed)))
        else {
            return Vec::new();
        };
        match self.read_pull_refs(mirror) {
            Ok(refs) => (refs.into_keys())
                .filter(|number| !handed.contains(number) && !opened.contains(number))
                .collect(),
            Err(cause) => {
                log::warn!("Tsugiki can't read which PR refs {repository}'s mirror keeps: {cause}");
                Vec::new()
            }
        }
    }

    /// Starts `git update-ref --stdin` to delete the refs of the PRs the plan prunes, or its task if it
    /// prunes none.
    fn start_upkeep_git(&mut self, plan: Plan) {
        let (step, input): (&[&str], String) = if plan.pruning.is_empty() {
            (plan.task.args(), String::new())
        } else {
            let deletes = (plan.pruning.iter())
                .map(|number| format!("delete refs/tsugiki/pull/{number}/head\n"))
                .collect();
            (&["update-ref", "--stdin"], deletes)
        };
        let in_mirror = [OsStr::new("-C"), plan.mirror.as_os_str()];
        let args: Vec<&OsStr> = in_mirror
            .into_iter()
            .chain(step.iter().map(OsStr::new))
            .collect();
        match self.git.start(&args, input.as_bytes()) {
            Ok(started) => {
                self.listen(started.output);
                self.job = Some(Job::Upkeep(Upkeep {
                    plan,
                    git: started.run,
                }));
            }
            Err(cause) => {
                let repository = &plan.repository;
                log::warn!("Tsugiki can't start the upkeep of {repository}'s mirror: {cause}");
                self.upkeep_done(&plan);
            }
        }
    }

    /// Goes on from the step of an upkeep that ended. A task that fails waits for its next turn, so one
    /// that can't work doesn't run again and again, unless git says the mirror is broken. After refs that
    /// went, the task waits for its turn behind the fetches and clones; after refs that couldn't, it starts.
    pub(super) fn finish_upkeep(&mut self, upkeep: Upkeep, last_words: &str) {
        let Upkeep { plan, mut git } = upkeep;
        let succeeded = git.succeeded();
        let repository = &plan.repository;
        if !succeeded
            && says_broken(last_words)
            && self.replace_broken(repository, &plan.mirror, last_words)
        {
            return;
        }
        if plan.pruning.is_empty() {
            if !succeeded {
                log::warn!("Tsugiki's upkeep of {repository}'s mirror failed: {last_words}");
            }
            self.upkeep_done(&plan);
            return;
        }
        if succeeded {
            // A PR that comes back in reach has its head fetched again.
            // Only a Repository with heads handed over has refs to prune.
            let known =
                (self.heads.iter_mut()).filter(|heads| heads.repository.is_same(repository));
            for known in known {
                for number in &plan.pruning {
                    drop(known.settled.remove(number));
                    drop(known.behind.remove(number));
                }
            }
            return;
        }
        log::warn!(
            "Tsugiki can't delete the refs of PRs gone from {repository}'s work: {last_words}"
        );
        self.start_upkeep_git(Plan {
            pruning: Vec::new(),
            ..plan
        });
    }

    /// Notes `task` done on `repository`'s mirror now, in its file too.
    fn upkeep_done(&mut self, plan: &Plan) {
        let now = SystemTime::now();
        let repository = &plan.repository;
        if let Some((_, upkept)) = (self.upkept.iter_mut()).find(|(one, _)| one.is_same(repository))
        {
            *upkept = upkept.after(plan.task, now);
            write_upkept(repository, &plan.mirror, *upkept);
        }
    }

    /// Notes a new clone of `repository` at `mirror`, whose upkeep starts afresh, and deletes the broken
    /// mirror it replaces, if there was one.
    pub(super) fn cloned_afresh(&mut self, repository: &RepositoryName, mirror: &Path) {
        let _deleted = delete_broken(repository, mirror);
        // The worker heard of each Repository whose mirror it keeps before it cloned it.
        if let Some((_, upkept)) = (self.upkept.iter_mut()).find(|(one, _)| one.is_same(repository))
        {
            *upkept = Upkept::at(SystemTime::now());
            write_upkept(repository, mirror, *upkept);
        }
    }

    /// Moves `repository`'s mirror aside, as git says it is broken, and clones it again ahead of the others.
    /// The heads fetched and the reviewed commits kept since launch wait for the new mirror, and the broken
    /// one goes once it is there.
    /// Says whether it moved the mirror: Windows refuses while another program has a file in it open, and
    /// the worker replaces each mirror once a launch, so words it misreads can't keep it cloning.
    pub(super) fn replace_broken(
        &mut self,
        repository: &RepositoryName,
        mirror: &Path,
        last_words: &str,
    ) -> bool {
        if self.replaced.iter().any(|one| one.is_same(repository)) {
            log::warn!(
                "Tsugiki leaves {repository}'s mirror, as it cloned it again since launch: {last_words}"
            );
            return false;
        }
        let broken = mirror.with_file_name(BROKEN);
        // One left by a replacement whose clone didn't finish goes first.
        if let Err(cause) = remove_if_there(&broken).and_then(|()| fs::rename(mirror, &broken)) {
            log::warn!(
                "Tsugiki can't move {repository}'s broken mirror aside, so it stays: {cause}"
            );
            return false;
        }
        log::warn!(
            "Tsugiki clones {repository}'s mirror again, as git says it is broken: {last_words}"
        );
        self.replaced.push(repository.clone());
        let mut known = self.take_heads(repository);
        let settled = mem::take(&mut known.settled);
        let behind =
            mem::take(&mut known.behind)
                .into_iter()
                .map(|(number, behind)| match behind {
                    Behind::Due { commit, .. } | Behind::Retried { commit } => (number, commit),
                });
        for (number, commit) in settled.into_iter().chain(behind) {
            let _ = known.waiting.entry(number).or_insert(commit);
        }
        known.reviews.keep_again();
        known.lookup.forget();
        self.heads.push_front(known);
        self.wait(repository.clone(), Turn::Next, None);
        true
    }
}
