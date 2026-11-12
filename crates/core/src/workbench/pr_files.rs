//! The PR view's Files tab: the PR's diff from its Repository's mirror, or why it isn't there yet (ADR
//! 0008). The window asks for the rows in chunks (ADR 0001).

mod threads;

use std::ops::Range;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::diff::{
    self, Against, Chunk, DiffFile, DiffOf, DiffRow, Diffs, FileLines, Found, HeadLine, Held,
    Highlighter, NewsEntry, OpenDiff, PrAt, Start,
};
use crate::error::{Error, ErrorKind};
use crate::folders::Folders;
use crate::github::CommitSha;
pub use crate::github::pull_request_content::CodeRow;
use crate::github::pull_request_content::PullRequestContent;
use crate::github::pull_requests::{PrHead, PrState, PullRequestFacts};
use crate::highlight::Span;
use crate::mirror::MirrorRequests;
use crate::repository::RepositoryName;
use crate::store::StoreReader;
pub use threads::{DiffThread, Placement, ThreadCode};

/// What the Files tab shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PrFiles {
    /// The Repository's mirror is still being cloned, and the clone says how far it is.
    Cloning,
    /// The Repository's mirror isn't cloned yet, and its clone waits for the user to say where it gets its
    /// objects.
    Asking,
    /// The mirror lacks the PR's commits, and is fetching them.
    Fetching,
    /// The diff, whose rows the window asks for by number.
    Ready {
        /// The commit the diff ends at, which each chunk names.
        #[specta(type = String)]
        head: CommitSha,
        /// The merge base the diff starts from, which each chunk names too. A PR moved to another
        /// Target branch keeps its head and gets another merge base (#211).
        #[specta(type = String)]
        base: CommitSha,
        files: Vec<DiffFile>,
        rows: u32,
        /// The PR's NEWS entries, rendered above the rows.
        news: Vec<NewsEntry>,
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
    },
}

/// Where the Files tab reads from: the database, the mirrors, the diff on show and the trees of its files.
/// It asks the mirrors' worker for a head the mirror lacks.
pub struct DiffSources<'a> {
    pub reader: &'a StoreReader,
    pub folders: &'a Folders,
    pub diffs: &'a Diffs,
    pub mirrors: &'a MirrorRequests,
    pub highlighter: &'a Highlighter,
}

impl PrFiles {
    /// What PR `number`'s Files tab shows, or nothing if Tsugiki doesn't hold it. A diff that isn't on show
    /// takes as long as git does, and a PR whose head the mirror lacks goes ahead of the heads waiting for
    /// a fetch. Until that head comes, the diff at the PR's last head the mirror fetched shows, if any.
    pub fn read(
        sources: &DiffSources<'_>,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<Self>, Error> {
        let Some((pr, content)) = facts(sources.reader, repository, number)? else {
            return Ok(None);
        };
        let mirror = sources.folders.mirror(repository);
        if !mirror.exists() {
            let asks = sources.mirrors.asks(repository);
            return Ok(Some(if asks { Self::Asking } else { Self::Cloning }));
        }
        sources.mirrors.opened(repository.clone(), number);
        // The sync reads a PR's head the next time it reads the PR.
        let Some(head) = pr.head.clone() else {
            return Ok(Some(Self::Fetching));
        };
        let at = PrAt {
            repository: repository.clone(),
            number,
            head: head.clone(),
        };
        let start = start(&pr);
        let target = Against::Branch(&pr.target_branch, &start);
        // The diff on show is in the mirror, so asking git again would only cost a moment.
        if let Some(open) = sources.diffs.shown(&at, target) {
            return Ok(Some(Self::ready(&open, content.as_ref(), None)));
        }
        match diff::held(&mirror, &pr.target_branch, &head) {
            Held::NoHead => {
                fetch_first(sources.mirrors, repository, &pr, &head);
                let older = diff::last_head(&mirror, number).filter(|older| {
                    matches!(diff::held(&mirror, &pr.target_branch, older), Held::Both)
                });
                let Some(older) = older else {
                    return Ok(Some(Self::Fetching));
                };
                let read = || diff::read(&mirror, &pr.target_branch, &start, &older);
                let at = PrAt {
                    head: older.clone(),
                    ..at
                };
                let open = sources.diffs.get_or_read(&at, target, read)?;
                Ok(Some(Self::ready(&open, content.as_ref(), Some(head))))
            }
            Held::NoTarget => Err(Error::new(
                ErrorKind::NotFound,
                &format!(
                    "The mirror of {repository} has no branch {}, which this PR targets.",
                    pr.target_branch
                ),
            )),
            Held::Both => {
                let read = || diff::read(&mirror, &pr.target_branch, &start, &head);
                let open = sources.diffs.get_or_read(&at, target, read)?;
                Ok(Some(Self::ready(&open, content.as_ref(), None)))
            }
        }
    }

    /// Hands PR `number`'s head to the mirrors' worker ahead of the heads waiting, if its mirror lacks it,
    /// as the user has opened the PR. It runs one git, which says whether the mirror holds the head, so a
    /// PR left on its Conversation costs no diff, and the head is there or on its way once the Files tab
    /// shows.
    pub fn fetch_missing_head(
        sources: &DiffSources<'_>,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<(), Error> {
        let Some((pr, _content)) = facts(sources.reader, repository, number)? else {
            return Ok(());
        };
        let mirror = sources.folders.mirror(repository);
        if !mirror.exists() {
            return Ok(());
        }
        sources.mirrors.opened(repository.clone(), number);
        // The sync reads a PR's head the next time it reads the PR.
        if let Some(head) = &pr.head
            && !diff::holds_commit(&mirror, head)
        {
            fetch_first(sources.mirrors, repository, &pr, head);
        }
        Ok(())
    }

    /// The diff `open`, with the threads of `content`. Their lines count in the head GitHub had when Tsugiki
    /// last read the Conversation, so a diff that ends elsewhere has them once the two agree. While the
    /// mirror fetches the PR's `newer` head, the tab only counts them.
    fn ready(
        open: &OpenDiff,
        content: Option<&PullRequestContent>,
        newer: Option<CommitSha>,
    ) -> Self {
        let threads = match newer {
            Some(newer) => {
                let count = content.map_or(0, |content| content.threads().count());
                DiffThreads::HeldBack {
                    newer,
                    count: u32::try_from(count).unwrap_or(u32::MAX),
                }
            }
            None => {
                let placed = content
                    .filter(|content| content.head.as_ref() == Some(&open.pr.head))
                    .map(|content| threads::place(&open.diff, content.threads()));
                DiffThreads::Shown {
                    threads: placed.unwrap_or_default(),
                }
            }
        };
        Self::Ready {
            head: open.pr.head.clone(),
            base: open.base.clone(),
            files: open.diff.files().to_vec(),
            rows: open.diff.row_count(),
            news: open.news.clone(),
            threads,
        }
    }

    /// The rows in `range` of the diff `of`, cut to its end. They come from the diff on show, unless the
    /// user has opened another PR since.
    pub fn rows(
        sources: &DiffSources<'_>,
        of: &DiffOf,
        range: Range<u32>,
    ) -> Result<Vec<DiffRow>, Error> {
        Ok(Self::open(sources, of)?.diff.rows(range).to_vec())
    }

    /// Where the code of the rows of the diff `of` holds `query`, whatever the case: in every row, those the
    /// window hasn't asked for too (#186).
    pub fn find(sources: &DiffSources<'_>, of: &DiffOf, query: &str) -> Result<Found, Error> {
        Ok(Self::open(sources, of)?.diff.find(query))
    }

    /// The spans of the rows in `range`, which highlight their code (#178). Those of a file the window shows
    /// collapsed come only once it opens the file, and `collapsed` names those, by their place among the
    /// files (#181).
    pub fn spans(
        sources: &DiffSources<'_>,
        of: &DiffOf,
        range: Range<u32>,
        collapsed: &[u32],
    ) -> Result<Vec<Vec<Span>>, Error> {
        let mirror = sources.folders.mirror(&of.pr.repository);
        let open = Self::open(sources, of)?;
        let chunk = Chunk {
            rows: range,
            collapsed,
        };
        Ok(sources.highlighter.spans(&open, &mirror, chunk))
    }

    /// Lines `lines` of file `file` at the PR's head, counted from 1, with their spans: those a gap of the
    /// diff `of` leaves out, which the user expands (#182). They come from the mirror, so expanding asks
    /// GitHub for nothing. Those past the file's end don't come.
    pub fn lines(
        sources: &DiffSources<'_>,
        of: &DiffOf,
        file: u32,
        lines: Range<u32>,
    ) -> Result<Vec<HeadLine>, Error> {
        let mirror = sources.folders.mirror(&of.pr.repository);
        let open = Self::open(sources, of)?;
        let file = usize::try_from(file).unwrap_or(usize::MAX);
        let wanted = FileLines { file, lines };
        Ok(sources.highlighter.head_lines(&open, &mirror, wanted))
    }

    /// The diff `of`: the one on show, or else read again from its base, as when the user has opened
    /// another PR since. The Target branch only says which files are Generated.
    fn open(sources: &DiffSources<'_>, of: &DiffOf) -> Result<Arc<OpenDiff>, Error> {
        let DiffOf { pr, base } = of;
        sources.diffs.get_or_read(pr, Against::Base(base), || {
            let facts = facts(sources.reader, &pr.repository, pr.number)?;
            let Some((facts, _content)) = facts else {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    &format!("Tsugiki doesn't hold PR #{}.", pr.number),
                ));
            };
            let mirror = sources.folders.mirror(&pr.repository);
            diff::read(
                &mirror,
                &facts.target_branch,
                &Start::Base(base.clone()),
                &pr.head,
            )
        })
    }
}

/// Where the diff of `pr` starts. A merged PR's starts where GitHub's does, as its head can be in the
/// Target branch by now. One stored before Tsugiki read where that is starts from its merge commit's
/// first parent, unless the merge was a fast-forward to its head (#212). A diff at an older head the
/// mirror holds starts there too.
fn start(pr: &PullRequestFacts) -> Start {
    if pr.state != PrState::Merged {
        return Start::TargetBranch;
    }
    match (&pr.base, &pr.merge_commit) {
        (Some(base), _) => Start::MergeBaseWith(base.clone()),
        (None, Some(merge)) if pr.head.as_ref() != Some(merge) => {
            Start::MergeBaseWithParentOf(merge.clone())
        }
        (None, _) => Start::TargetBranch,
    }
}

/// Hands `head`, the head of `pr` its mirror lacks, to the mirrors' worker ahead of the heads waiting.
fn fetch_first(
    mirrors: &MirrorRequests,
    repository: &RepositoryName,
    pr: &PullRequestFacts,
    head: &CommitSha,
) {
    let head = PrHead {
        number: pr.number,
        commit: head.clone(),
        target_branch: pr.target_branch.clone(),
    };
    mirrors.fetch_first(repository.clone(), head);
}

/// PR `number`'s facts, and its Conversation once Tsugiki has read it, as the database last had them.
fn facts(
    reader: &StoreReader,
    repository: &RepositoryName,
    number: u32,
) -> Result<Option<(PullRequestFacts, Option<PullRequestContent>)>, Error> {
    let (facts, content) = reader.pull_request_facts(repository, number)?;
    let pr = facts
        .pull_requests
        .into_iter()
        .find(|pr| pr.number == number);
    Ok(pr.map(|pr| (pr, content)))
}

#[cfg(test)]
mod tests;
