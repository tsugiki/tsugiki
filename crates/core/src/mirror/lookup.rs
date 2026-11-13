//! Each Repository's **Lookup checkout** (ADR 0020): a detached worktree of the mirror, which the worker
//! keeps at the base of the PR on screen, for the language servers and text search to read.
//!
//! The worker makes it on the first PR that opens and moves it as each PR opens, in the background and
//! without asking: neither a person nor an editor writes to it. A move resets it to the PR's base, then
//! writes the PR's added files into it, as ty finds imports on disk. The next move's reset removes them.
//! Readers ask the worker where it is, and wait while it moves. git's gc keeps the commit at its HEAD, as it
//! keeps each worktree's.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use super::upkeep::remove_if_there;
use super::{Job, MirrorProblem, Run, Worker};
use crate::github::CommitSha;
use crate::repository::RepositoryName;

/// The two ends of the PR on screen, as its diff has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupAt {
    /// The commit the diff starts from, the merge base, which the checkout holds.
    pub base: CommitSha,
    /// The PR's head, whose added files the checkout holds too.
    pub head: CommitSha,
}

/// Where a Repository's Lookup checkout is, once no move is under way or due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupCheckout {
    pub folder: PathBuf,
    pub at: LookupAt,
}

/// What the worker knows of one Repository's Lookup checkout.
#[derive(Debug, Default)]
pub(super) struct Lookup {
    /// Where the last move left the checkout.
    left: Left,
    /// Where the PR on screen wants it, when that is somewhere else and no move there is under way.
    wanted: Option<LookupAt>,
    /// The readers waiting for the moves under way and due to end.
    readers: Vec<Sender<Option<LookupCheckout>>>,
}

impl Lookup {
    /// Forgets where the checkout is, as when its mirror is cloned again: git's note of the worktree went
    /// with the old mirror, so the next move makes it afresh.
    pub(super) fn forget(&mut self) {
        self.left = Left::Unknown;
    }
}

/// Where the last move left a Lookup checkout.
#[derive(Debug, Default)]
enum Left {
    /// No move has ended since launch, so the checkout may be anywhere, if it is there at all.
    #[default]
    Unknown,
    At(LookupAt),
    /// The move to this PR failed. The PR on screen asks for no other until a reader asks, so a cause that
    /// lasts doesn't start a move at each read of the Files tab. The next move makes the checkout afresh.
    Failed(LookupAt),
}

impl Left {
    fn at(&self) -> Option<&LookupAt> {
        match self {
            Self::At(at) => Some(at),
            Self::Unknown | Self::Failed(_) => None,
        }
    }
}

/// A move of a Lookup checkout under way: one git for each step.
pub(super) struct Move {
    trip: Trip,
    step: Step,
    pub(super) git: Box<dyn Run>,
}

/// Where a move takes a Repository's Lookup checkout, and the folders it reads and writes.
struct Trip {
    repository: RepositoryName,
    to: LookupAt,
    mirror: PathBuf,
    folder: PathBuf,
    /// The move made the checkout, so a reset that fails has nothing left to try.
    made: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// Adding the worktree, with nothing in it yet, when there is none or the last move failed.
    Make,
    /// Resetting it to the PR's base, which removes the last PR's added files with the rest.
    Reset,
    /// Writing the PR's added files, which aren't at the base, from its head.
    Add,
}

impl Worker {
    /// Moves `repository`'s Lookup checkout to `at` once the move under way, if any, has ended. A newer
    /// `at` replaces one that hasn't started yet.
    pub(super) fn look_up(&mut self, repository: &RepositoryName, at: LookupAt) {
        let moving = self.moving(repository).cloned();
        self.change_heads(repository, |known| {
            let lookup = &mut known.lookup;
            let failed = match &lookup.left {
                Left::Failed(failed) => Some(failed),
                Left::Unknown | Left::At(_) => None,
            };
            let there = moving.as_ref().or(lookup.left.at()).or(failed) == Some(&at);
            lookup.wanted = if there { None } else { Some(at) };
        });
    }

    /// Hands `reply` where `repository`'s Lookup checkout is, at once if no move is under way or due, or
    /// else once they have ended. A move that failed is tried once more. A worker that can't run git, or
    /// has no folders, moves nothing, so it answers at once with where the checkout was.
    pub(super) fn lookup_checkout(
        &mut self,
        repository: &RepositoryName,
        reply: Sender<Option<LookupCheckout>>,
    ) {
        let stuck = self.folders.is_err()
            || matches!(
                self.problem,
                Some(MirrorProblem::NoGit | MirrorProblem::OldGit { .. })
            );
        let moving = self.moving(repository).is_some();
        self.change_heads(repository, |known| {
            let lookup = &mut known.lookup;
            lookup.readers.push(reply);
            if stuck {
                lookup.wanted = None;
            } else if let (Left::Failed(failed), None, false) =
                (&lookup.left, &lookup.wanted, moving)
            {
                lookup.wanted = Some(failed.clone());
            }
        });
        self.answer_readers(repository);
    }

    /// Starts the next move a Lookup checkout waits for, in a Repository whose mirror is there, unless a git
    /// is under way. Short of disk, it only moves a checkout that is there, as making one writes as many
    /// files as the Repository has.
    pub(super) fn start_lookup_move(&mut self) {
        let (None, Ok(folders)) = (&self.job, &self.folders) else {
            return;
        };
        let found = self.heads.iter_mut().find(|known| {
            known.lookup.wanted.is_some() && folders.mirror(&known.repository).exists()
        });
        let Some((repository, Some(to), failed)) = found.map(|known| {
            let lookup = &mut known.lookup;
            let failed = matches!(lookup.left, Left::Failed(_));
            (known.repository.clone(), lookup.wanted.take(), failed)
        }) else {
            return;
        };
        let trip = Trip {
            mirror: folders.mirror(&repository),
            folder: folders.lookup(&repository),
            repository,
            to,
            made: false,
        };
        let there = trip.folder.join(".git").is_file();
        let step = if there && !failed {
            Step::Reset
        } else {
            Step::Make
        };
        if step == Step::Make && matches!(self.problem, Some(MirrorProblem::DiskShort { .. })) {
            let Trip { repository, to, .. } = trip;
            log::warn!("Tsugiki doesn't make {repository}'s Lookup checkout, as the disk is short");
            self.moved(&repository, Left::Failed(to));
            return;
        }
        self.start_move_step(trip, step);
    }

    /// Starts the git of `step`. A PR that adds nothing has no Add step, so its move ends there.
    fn start_move_step(&mut self, mut trip: Trip, step: Step) {
        let input = match step {
            Step::Add => self.added(&trip),
            Step::Make | Step::Reset => Vec::new(),
        };
        if step == Step::Add && input.is_empty() {
            self.moved(&trip.repository, Left::At(trip.to));
            return;
        }
        if step == Step::Make {
            trip.made = true;
            if let Err(cause) = remove_if_there(&trip.folder) {
                let Trip { repository, to, .. } = trip;
                log::warn!(
                    "Tsugiki can't clear the folder of {repository}'s Lookup checkout: {cause}"
                );
                self.moved(&repository, Left::Failed(to));
                return;
            }
        }
        let Trip {
            to, mirror, folder, ..
        } = &trip;
        // The mirror's own hooks folder holds none, and a filter such as git-lfs's smudge would ask the
        // network for files, which a move doesn't need. The user's `core.autocrlf` would give the servers
        // line endings the mirror's diff doesn't have.
        let mut hooks = OsString::from("core.hooksPath=");
        hooks.push(mirror.join("hooks"));
        let quiet = [
            "-c",
            "core.autocrlf=false",
            "-c",
            "filter.lfs.smudge=",
            "-c",
            "filter.lfs.process=",
            "-c",
        ];
        let config = (quiet.map(OsStr::new).into_iter()).chain([
            OsStr::new("filter.lfs.required=false"),
            OsStr::new("-c"),
            &hooks,
        ]);
        let (base, head) = (OsStr::new(to.base.as_str()), OsStr::new(to.head.as_str()));
        let args: Vec<&OsStr> = match step {
            // `--force` adds it where git still lists a worktree whose folder has gone.
            Step::Make => config
                .chain([OsStr::new("-C"), mirror.as_os_str()])
                .chain(["worktree", "add", "--force", "--detach", "--no-checkout"].map(OsStr::new))
                .chain([folder.as_os_str(), base])
                .collect(),
            Step::Reset => config
                .chain([OsStr::new("-C"), folder.as_os_str()])
                .chain(["reset", "--hard", "--quiet", "--no-recurse-submodules"].map(OsStr::new))
                .chain([base])
                .collect(),
            // The paths are names, not patterns, and stdin holds as many as the PR adds.
            Step::Add => config
                .chain([
                    OsStr::new("--literal-pathspecs"),
                    OsStr::new("-C"),
                    folder.as_os_str(),
                ])
                .chain([OsStr::new("checkout"), head])
                .chain(["--pathspec-from-file=-", "--pathspec-file-nul"].map(OsStr::new))
                .collect(),
        };
        match self.git.start(&args, &input) {
            Ok(started) => {
                self.listen(started.output);
                self.job = Some(Job::Lookup(Move {
                    trip,
                    step,
                    git: started.run,
                }));
            }
            Err(cause) => {
                let Trip { repository, to, .. } = trip;
                log::warn!("Tsugiki can't start moving {repository}'s Lookup checkout: {cause}");
                self.moved(&repository, Left::Failed(to));
            }
        }
    }

    /// Goes on to the move's next step, or notes where the move left the checkout. A reset that fails in a
    /// checkout the move didn't make, as when git's note of it has gone, makes it afresh. git's last words
    /// say why a step failed.
    pub(super) fn finish_lookup_move(&mut self, moving: Move, last_words: &str) {
        let Move {
            trip,
            step,
            mut git,
        } = moving;
        if !git.succeeded() {
            if step == Step::Reset && !trip.made {
                let repository = &trip.repository;
                log::info!("Tsugiki makes {repository}'s Lookup checkout afresh: {last_words}");
                self.start_move_step(trip, Step::Make);
            } else {
                let Trip { repository, to, .. } = trip;
                log::warn!("Tsugiki couldn't move {repository}'s Lookup checkout: {last_words}");
                self.moved(&repository, Left::Failed(to));
            }
            return;
        }
        match step {
            Step::Make => self.start_move_step(trip, Step::Reset),
            Step::Reset => self.start_move_step(trip, Step::Add),
            Step::Add => self.moved(&trip.repository, Left::At(trip.to)),
        }
    }

    /// The paths the PR adds, each ending in a NUL, as `git checkout --pathspec-file-nul` reads them. A
    /// renamed file counts as added under its new name. A failed read leaves the checkout at the base alone.
    fn added(&self, trip: &Trip) -> Vec<u8> {
        let Trip {
            repository,
            to,
            mirror,
            ..
        } = trip;
        let diff = [
            "diff",
            "--name-only",
            "--no-renames",
            "--diff-filter=A",
            "-z",
        ];
        let args: Vec<&OsStr> = [OsStr::new("-C"), mirror.as_os_str()]
            .into_iter()
            .chain(diff.map(OsStr::new))
            .chain([to.base.as_str(), to.head.as_str()].map(OsStr::new))
            .collect();
        match self.git.read(&args) {
            Ok(names) => names.into_bytes(),
            Err(cause) => {
                log::warn!("Tsugiki can't read which files a PR adds to {repository}: {cause}");
                Vec::new()
            }
        }
    }

    /// Notes where a move left `repository`'s Lookup checkout, and answers the readers if no move is due.
    fn moved(&mut self, repository: &RepositoryName, left: Left) {
        self.change_heads(repository, |known| known.lookup.left = left);
        self.answer_readers(repository);
    }

    /// Answers the readers of `repository`'s Lookup checkout, unless a move is under way or due. A move
    /// waits for its mirror, and the checkout can't be read without it, so without a mirror the readers
    /// hear there is none.
    fn answer_readers(&mut self, repository: &RepositoryName) {
        if self.moving(repository).is_some() {
            return;
        }
        let folders = self.folders.as_ref().ok();
        let mirrored = folders.is_some_and(|folders| folders.mirror(repository).exists());
        let folder = folders.map(|folders| folders.lookup(repository));
        self.change_heads(repository, |known| {
            let lookup = &mut known.lookup;
            if mirrored && lookup.wanted.is_some() {
                return;
            }
            let at = lookup.left.at().filter(|_| mirrored).cloned();
            let checkout = folder
                .zip(at)
                .map(|(folder, at)| LookupCheckout { folder, at });
            for reader in lookup.readers.drain(..) {
                // A reader that has stopped waiting needs no answer.
                let _unheard = reader.send(checkout.clone());
            }
        });
    }

    /// Where the move under way takes `repository`'s Lookup checkout, if one is.
    fn moving(&self, repository: &RepositoryName) -> Option<&LookupAt> {
        match &self.job {
            Some(Job::Lookup(moving)) if moving.trip.repository.is_same(repository) => {
                Some(&moving.trip.to)
            }
            Some(
                Job::Lookup(_) | Job::Clone(_) | Job::Fetch(_) | Job::Reviewed(_) | Job::Upkeep(_),
            )
            | None => None,
        }
    }
}
