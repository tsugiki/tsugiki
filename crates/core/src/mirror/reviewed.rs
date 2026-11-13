//! The commit each of the user's Reviews covered, kept under `refs/tsugiki/reviewed/N` (#382), so a Stale
//! review check finds both sides in the mirror. A reviewed ref moves only when the user's Review moves to
//! another commit: upkeep deletes the refs of PR heads alone, so gc keeps each reviewed commit.
//!
//! A commit the mirror lacks, from a Review older than the mirror or on a commit a force-push dropped before
//! the mirror fetched it, is fetched by its SHA, once. One GitHub refuses to send is noted in a file beside
//! the mirror and never asked for again.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::mem;
use std::path::{Path, PathBuf};

use super::{Job, Pace, Run, Worker, fetch_args, says_broken};
use crate::github::CommitSha;
use crate::github::pull_requests::ReviewedCommit;
use crate::repository::RepositoryName;

/// Where the reviewed refs go, each named by its PR's number.
const REVIEWED_REFS: &str = "refs/tsugiki/reviewed/";

/// The file beside a mirror, in its Repository's folder, that lists the commits GitHub refused to send, a
/// SHA on each line. It outlives a mirror cloned again, as GitHub won't have those commits then either.
const REFUSED_FILE: &str = "refused-commits.txt";

/// What GitHub says before the SHA of a commit it won't send: "not our ref" over git's protocol v2, and
/// "unadvertised object" over the older one.
const REFUSALS: [&str; 2] = [
    "upload-pack: not our ref",
    "request for unadvertised object",
];

/// The most commits one `git rev-list` looks for, so its command line stays well inside what Windows takes.
const LOOKS_AT_ONCE: usize = 500;

/// What the worker knows of one Repository's reviewed commits.
#[derive(Debug, Default)]
pub(super) struct Reviews {
    /// Commits handed over, by PR, that their refs may not point at yet.
    waiting: BTreeMap<u32, CommitSha>,
    /// Where the worker pointed each reviewed ref since launch.
    kept: BTreeMap<u32, CommitSha>,
    /// The commits GitHub refused to send, read from the file beside the mirror before the first keeping.
    refused: Option<BTreeSet<CommitSha>>,
}

impl Reviews {
    /// The commits GitHub refused to send, read from the file beside `mirror` the first time.
    pub(super) fn refused(
        &mut self,
        repository: &RepositoryName,
        mirror: &Path,
    ) -> &BTreeSet<CommitSha> {
        self.refused
            .get_or_insert_with(|| read_refused(repository, mirror))
    }

    /// Keeps each reviewed commit again, in a mirror cloned anew.
    pub(super) fn keep_again(&mut self) {
        let kept = mem::take(&mut self.kept);
        self.put_back(kept);
    }

    /// Notes the commits now under their refs. One handed over again meanwhile asks for nothing more.
    fn note_kept(&mut self, kept: BTreeMap<u32, CommitSha>) {
        for (number, commit) in kept {
            if self.waiting.get(&number) == Some(&commit) {
                drop(self.waiting.remove(&number));
            }
            drop(self.kept.insert(number, commit));
        }
    }

    /// Puts back commits that didn't reach their refs, behind any handed over meanwhile.
    fn put_back(&mut self, commits: BTreeMap<u32, CommitSha>) {
        for (number, commit) in commits {
            let _ = self.waiting.entry(number).or_insert(commit);
        }
    }
}

/// A git keeping a Repository's reviewed commits.
pub(super) struct KeepReviewed {
    repository: RepositoryName,
    mirror: PathBuf,
    step: Step,
    pub(super) git: Box<dyn Run>,
    /// Commits or a status came while it ran, so if its fetch fails, the next one has a reason to try again.
    pub(super) news: bool,
}

/// The two steps of keeping, a git each.
enum Step {
    /// Pointing the reviewed refs at the commits in `present`, which the mirror has. Those in `missing` are
    /// fetched next.
    Point {
        present: BTreeMap<u32, CommitSha>,
        missing: BTreeMap<u32, CommitSha>,
    },
    /// Fetching the commits in `missing` by their SHAs, straight into their reviewed refs.
    Fetch { missing: BTreeMap<u32, CommitSha> },
}

impl Worker {
    /// Keeps each commit of `reviewed` that its ref may not point at yet, for the next keeping.
    pub(super) fn hand_over_reviewed(
        &mut self,
        repository: &RepositoryName,
        reviewed: Vec<ReviewedCommit>,
    ) {
        self.change_reviews(repository, |reviews| {
            for ReviewedCommit { number, commit } in reviewed {
                if reviews.kept.get(&number) == Some(&commit) {
                    drop(reviews.waiting.remove(&number));
                } else {
                    drop(reviews.waiting.insert(number, commit));
                }
            }
        });
    }

    /// Changes what the worker knows of `repository`'s reviewed commits with `change`. The Repository keeps
    /// its turn for heads.
    fn change_reviews(&mut self, repository: &RepositoryName, change: impl FnOnce(&mut Reviews)) {
        self.change_heads(repository, |known| change(&mut known.reviews));
    }

    /// Starts keeping the reviewed commits that wait in the first Repository whose mirror is there, while
    /// the worker is going. Says whether it took any, as it may start no git, and the caller then looks for
    /// the next thing to do.
    pub(super) fn start_keeping_reviewed(&mut self) -> bool {
        let Ok(folders) = self.folders.as_ref() else {
            return false;
        };
        if self.pace != Pace::Going {
            return false;
        }
        let found = self.heads.iter_mut().find(|known| {
            !known.reviews.waiting.is_empty() && folders.mirror(&known.repository).exists()
        });
        let Some(known) = found else {
            return false;
        };
        let repository = known.repository.clone();
        let mirror = folders.mirror(&repository);
        let reviews = &mut known.reviews;
        let mut waiting = mem::take(&mut reviews.waiting);
        let refused = reviews.refused(&repository, &mirror);
        waiting.retain(|_, commit| !refused.contains(commit));
        if waiting.is_empty() {
            return true;
        }
        // Each comes back at the next hand-over.
        let present = match self.present(&mirror, &waiting) {
            Ok(present) => present,
            Err(cause) => {
                log::warn!(
                    "Tsugiki can't read which reviewed commits {repository}'s mirror has: {cause}"
                );
                return true;
            }
        };
        let (present, missing): (BTreeMap<_, _>, BTreeMap<_, _>) =
            (waiting.into_iter()).partition(|(_, commit)| present.contains(commit));
        if present.is_empty() {
            self.start_reviewed_fetch(repository, mirror, missing);
        } else {
            self.start_step(repository, mirror, Step::Point { present, missing });
        }
        true
    }

    /// The commits of `commits` that `mirror` has.
    fn present(
        &self,
        mirror: &Path,
        commits: &BTreeMap<u32, CommitSha>,
    ) -> io::Result<BTreeSet<CommitSha>> {
        let shas: Vec<&OsStr> = commits
            .values()
            .map(|commit| OsStr::new(commit.as_str()))
            .collect();
        let mut present = BTreeSet::new();
        for some in shas.chunks(LOOKS_AT_ONCE) {
            let look = [
                OsStr::new("-C"),
                mirror.as_os_str(),
                OsStr::new("rev-list"),
                OsStr::new("--no-walk"),
                OsStr::new("--ignore-missing"),
            ];
            let args: Vec<&OsStr> = look.into_iter().chain(some.iter().copied()).collect();
            present.extend(self.git.read(&args)?.lines().filter_map(CommitSha::parse));
        }
        Ok(present)
    }

    /// Fetches each commit in `missing` by its SHA into its reviewed ref, all in one go, if there are any.
    fn start_reviewed_fetch(
        &mut self,
        repository: RepositoryName,
        mirror: PathBuf,
        missing: BTreeMap<u32, CommitSha>,
    ) {
        if !missing.is_empty() {
            self.start_step(repository, mirror, Step::Fetch { missing });
        }
    }

    /// Starts the git of `step` in `mirror`: `git update-ref` to point refs at commits the mirror has, or
    /// `git fetch` with a refspec from each SHA to its ref.
    fn start_step(&mut self, repository: RepositoryName, mirror: PathBuf, step: Step) {
        let (args, input): (Vec<&OsStr>, String) = match &step {
            Step::Point { present, .. } => {
                let args = vec![
                    OsStr::new("-C"),
                    mirror.as_os_str(),
                    OsStr::new("update-ref"),
                    OsStr::new("--stdin"),
                ];
                let updates = (present.iter())
                    .map(|(number, commit)| {
                        format!("update {REVIEWED_REFS}{number} {}\n", commit.as_str())
                    })
                    .collect();
                (args, updates)
            }
            Step::Fetch { missing } => {
                let refspecs = (missing.iter())
                    .map(|(number, commit)| {
                        format!("+{}:{REVIEWED_REFS}{number}\n", commit.as_str())
                    })
                    .collect();
                (fetch_args(&mirror, &[]), refspecs)
            }
        };
        let started = self.git.start(&args, input.as_bytes());
        match (started, step) {
            (Ok(started), step) => {
                self.listen(started.output);
                self.job = Some(Job::Reviewed(KeepReviewed {
                    repository,
                    mirror,
                    step,
                    git: started.run,
                    news: false,
                }));
            }
            // The commits in `present` come back at the next hand-over.
            (Err(cause), Step::Point { missing, .. }) => {
                log::warn!(
                    "Tsugiki can't start keeping the reviewed commits of {repository}: {cause}"
                );
                self.start_reviewed_fetch(repository, mirror, missing);
            }
            (Err(cause), Step::Fetch { missing }) => {
                log::warn!(
                    "Tsugiki can't start fetching the reviewed commits of {repository}: {cause}"
                );
                self.put_back_reviewed(&repository, missing);
                self.pace = Pace::Stalled;
            }
        }
    }

    /// Notes the commits a step brought under their refs, and goes on to the next step.
    ///
    /// GitHub names the commit it refuses to send, in one of the `REFUSALS`. That commit is noted as
    /// refused, and the others are fetched again at once. A fetch that fails for another reason puts its
    /// commits back, and waits for news, as a fetch of heads does.
    pub(super) fn finish_reviewed(&mut self, keeping: KeepReviewed, last_words: &str) {
        let KeepReviewed {
            repository,
            mirror,
            step,
            mut git,
            news,
        } = keeping;
        let succeeded = git.succeeded();
        let broken = !succeeded && says_broken(last_words);
        match step {
            Step::Point { present, missing } => {
                if succeeded {
                    self.note_kept_reviewed(&repository, present);
                } else if broken && self.replace_broken(&repository, &mirror, last_words) {
                    self.put_back_reviewed(&repository, present);
                    self.put_back_reviewed(&repository, missing);
                    return;
                } else {
                    // They come back at the next hand-over.
                    log::warn!(
                        "Tsugiki can't point {repository}'s reviewed refs at their commits: {last_words}"
                    );
                }
                self.start_reviewed_fetch(repository, mirror, missing);
            }
            Step::Fetch { missing } if succeeded => {
                self.note_kept_reviewed(&repository, missing);
                // The Stale review check can read the commits it brought now.
                self.change_heads(&repository, |known| known.patches.read_missing_again());
            }
            Step::Fetch { mut missing } => {
                let refusal = REFUSALS.iter().any(|words| last_words.contains(words));
                let refused: Vec<(u32, CommitSha)> = (missing.iter())
                    .filter(|(_, commit)| refusal && last_words.contains(commit.as_str()))
                    .map(|(number, commit)| (*number, commit.clone()))
                    .collect();
                for (number, _) in &refused {
                    drop(missing.remove(number));
                }
                self.put_back_reviewed(&repository, missing);
                if !refused.is_empty() {
                    self.note_refused(&repository, &mirror, refused);
                    return;
                }
                if broken && self.replace_broken(&repository, &mirror, last_words) {
                    return;
                }
                log::warn!(
                    "Tsugiki couldn't fetch the reviewed commits of {repository}: {last_words}"
                );
                if !news {
                    self.pace = Pace::Stalled;
                }
            }
        }
    }

    fn note_kept_reviewed(&mut self, repository: &RepositoryName, kept: BTreeMap<u32, CommitSha>) {
        self.change_reviews(repository, |reviews| reviews.note_kept(kept));
    }

    fn put_back_reviewed(
        &mut self,
        repository: &RepositoryName,
        commits: BTreeMap<u32, CommitSha>,
    ) {
        self.change_reviews(repository, |reviews| reviews.put_back(commits));
    }

    /// Notes that GitHub refused to send the commits in `refused`, in the file beside `mirror` too, so they
    /// aren't asked for again.
    fn note_refused(
        &mut self,
        repository: &RepositoryName,
        mirror: &Path,
        refused: Vec<(u32, CommitSha)>,
    ) {
        let mut lines = String::new();
        for (number, commit) in &refused {
            let sha = commit.as_str();
            log::warn!(
                "GitHub refused to send commit {sha} of {repository}, which the Review of PR #{number} covered"
            );
            lines.push_str(sha);
            lines.push('\n');
        }
        self.change_reviews(repository, |reviews| {
            let noted = reviews.refused.get_or_insert_with(BTreeSet::new);
            noted.extend(refused.into_iter().map(|(_, commit)| commit));
        });
        if let Err(cause) = append_refused(mirror, &lines) {
            log::warn!(
                "Tsugiki can't note the commits GitHub refused to send for {repository}: {cause}"
            );
        }
    }
}

/// The commits the file beside `mirror` says GitHub refused to send.
fn read_refused(repository: &RepositoryName, mirror: &Path) -> BTreeSet<CommitSha> {
    match fs::read_to_string(mirror.with_file_name(REFUSED_FILE)) {
        Ok(text) => text.lines().filter_map(CommitSha::parse).collect(),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(cause) => {
            log::warn!(
                "Tsugiki can't read which commits GitHub refused to send for {repository}: {cause}"
            );
            BTreeSet::new()
        }
    }
}

fn append_refused(mirror: &Path, lines: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(mirror.with_file_name(REFUSED_FILE))?;
    file.write_all(lines.as_bytes())
}
