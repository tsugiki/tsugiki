//! The calls the window's screens make into the core (ADR 0023). A call reads what the core holds through a
//! view model in `workbench`, whose read sends nothing, queues through `fetching` a job for what the read
//! found missing, and answers. An answer that says its data is on the way holds the job that brings it.

use serde::Serialize;
use specta::Type;

use crate::definitions::Definitions;
use crate::error::Error;
use crate::facts::CommitSha;
use crate::facts::pull_requests::HeadInMirror;
use crate::fetching::{Fetching, Queued};
use crate::mirror::LookupAt;
use crate::repository::RepositoryName;
use crate::workbench::pr_files::{DiffSources, DiffThread, FilesRead, OlderDiff, ShownDiff};

/// What the Files tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
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
    /// The mirror lacks the PR's commits, and is fetching them, or Tsugiki is reading the Conversation that
    /// names the PR's head.
    Fetching {
        #[serde(skip)]
        #[specta(skip)]
        fetch: Queued,
    },
    /// The diff, whose rows the window asks for by number.
    Ready {
        #[serde(flatten)]
        diff: ShownDiff,
        threads: DiffThreads,
    },
}

/// The review threads the Files tab shows with a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
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

/// What the calls of a PR's screens read from and ask: where the Files tab reads, the queues behind
/// `fetching`, and the language servers, which open the files of the diff on show.
pub struct PullRequestWork<'a> {
    pub sources: DiffSources<'a>,
    pub fetching: &'a Fetching,
    pub definitions: &'a Definitions,
}

impl PullRequestWork<'_> {
    /// The user opened PR `number`: the session reads its Conversation from GitHub, each time, and the Mirror
    /// worker fetches its head first if it lacks it. It waits for one git, which says whether the mirror
    /// holds the head. Its Last seen stays.
    pub fn open_pull_request(&self, repository: &RepositoryName, number: u32) -> Result<(), Error> {
        self.fetching.fetch_pull_request(repository.clone(), number);
        if let Some(head) = self.sources.head_in_mirror(repository, number)? {
            self.fetching
                .pull_request_opened(repository.clone(), number, head);
        }
        Ok(())
    }

    /// What PR `number`'s Files tab shows, or nothing if Tsugiki doesn't hold it, once the job that brings
    /// what the read found missing is queued. A diff that isn't on show waits on git, and never on GitHub.
    pub fn pr_files(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<PrFiles>, Error> {
        let asks = || self.fetching.mirror_asks(repository);
        let read = self.sources.read(repository, number, &asks)?;
        Ok(read.map(|read| self.answer(repository, number, read)))
    }

    /// What the Files tab of PR `number` shows of `read`. Each answer that waits for data queues the job
    /// that brings it.
    fn answer(&self, repository: &RepositoryName, number: u32, read: FilesRead) -> PrFiles {
        let fetching = self.fetching;
        match read {
            FilesRead::Asking => PrFiles::Asking,
            FilesRead::NoMirror => PrFiles::Cloning {
                clone: fetching.clone_shown(repository.clone()),
            },
            FilesRead::NoHead => PrFiles::Fetching {
                fetch: fetching.find_head(repository.clone(), number),
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
        }
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
