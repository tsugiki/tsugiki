//! The PR view's Files tab: what a read finds of the PR's diff in its Repository's mirror, and the rows the
//! window asks for in chunks (ADR 0001). The core's call in `calls` answers the tab from the read, once it
//! has queued what the read found missing (ADR 0023).

mod threads;

use std::ops::Range;
use std::sync::Arc;

use serde::Serialize;
use specta::Type;

use crate::diff::{
    self, Against, Chunk, DiffEnd, DiffFile, DiffOf, DiffRow, Diffs, FileLines, Found, HeadLine,
    Held, Highlighter, NewsEntry, OpenDiff, PrAt, Start,
};
use crate::error::{Error, ErrorKind};
use crate::facts::CommitSha;
pub use crate::facts::pull_request_content::CodeRow;
use crate::facts::pull_request_content::Side;
use crate::facts::pull_requests::{HeadInMirror, PrHead, PrState, PullRequestFacts};
use crate::folders::Folders;
use crate::highlight::Span;
use crate::repository::RepositoryName;
use crate::store::StoreReader;
pub use threads::{DiffThread, Placement, ThreadCode};

/// A diff the Files tab shows: its ends, its files, how many rows it has, and the PR's NEWS entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct ShownDiff {
    /// The commit the diff ends at, which each chunk names.
    #[specta(type = String)]
    pub head: CommitSha,
    /// The merge base the diff starts from, which each chunk names too. A PR moved to another Target branch
    /// keeps its head and gets another merge base (#211).
    #[specta(type = String)]
    pub base: CommitSha,
    pub files: Vec<DiffFile>,
    pub rows: u32,
    /// The PR's NEWS entries, rendered above the rows.
    pub news: Vec<NewsEntry>,
}

/// A line of a file of a PR's diff: its path at `side`, which is a renamed file's old path at the base, and
/// its number there, from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLine {
    pub path: String,
    pub side: DiffEnd,
    pub line: u32,
}

/// A line of one of the diff's files as the diff on show has it, which Go to definition goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineInDiff {
    /// Row `row` of the diff shows it (#444).
    Row { row: u32 },
    /// No hunk shows it: it is line `head_line` at the head of file `file`, by its place among the diff's
    /// files, in a gap or after the file's last hunk, which the window opens around it, as around a thread
    /// on a line no hunk shows (#445). The diff doesn't know where a file ends at the head, so a line past
    /// its end comes out here too, and the window, which reads the lines, finds it missing.
    Hidden { file: u32, head_line: u32 },
}

/// Where the Files tab reads from: the database, the mirrors, the diff on show and the trees of its files.
/// It asks nothing of anyone: the core's call queues what a read finds missing.
pub(crate) struct DiffSources<'a> {
    pub reader: &'a StoreReader,
    pub folders: &'a Folders,
    pub diffs: &'a Diffs,
    pub highlighter: &'a Highlighter,
}

/// What a read of a PR's Files tab found. Where the Mirror lacks what the tab shows, the read says what, so
/// the core's call can queue the job that brings it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesRead {
    /// The Repository's mirror isn't cloned yet, and its clone waits for the user to say where it gets its
    /// objects.
    Asking,
    /// The Repository's mirror isn't cloned yet.
    NoMirror,
    /// Tsugiki knows no head of the PR: neither the sync nor a Conversation it saved named one.
    NoHead,
    /// The Mirror lacks the PR's head, and holds the diff at the last head of the PR it fetched, if any.
    Missing {
        head: PrHead,
        older: Option<OlderDiff>,
    },
    /// The diff at the PR's head, which the Mirror holds, with the threads of the PR's Conversation that
    /// count their lines in it.
    Held {
        diff: ShownDiff,
        threads: Vec<DiffThread>,
    },
}

/// The diff at an older head of a PR, which shows while the Mirror lacks the newer one. The threads' lines
/// count in the newer head, so the tab holds back all `threads` of them until its diff comes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OlderDiff {
    pub diff: ShownDiff,
    pub threads: u32,
}

impl DiffSources<'_> {
    /// What PR `number`'s Files tab finds, or nothing if Tsugiki doesn't hold the PR. A diff that isn't on
    /// show takes as long as git does. A Repository whose mirror isn't there yet finds its clone, which asks
    /// where the mirror gets its objects while `asks` says it waits for that.
    pub fn read(
        &self,
        repository: &RepositoryName,
        number: u32,
        asks: &dyn Fn() -> bool,
    ) -> Result<Option<FilesRead>, Error> {
        let Some((pr, content)) = self.reader.pull_request(repository, number)? else {
            return Ok(None);
        };
        let mirror = self.folders.mirror(repository);
        if !mirror.exists() {
            return Ok(Some(if asks() {
                FilesRead::Asking
            } else {
                FilesRead::NoMirror
            }));
        }
        let Some(head) = pr.head.clone() else {
            return Ok(Some(FilesRead::NoHead));
        };
        let at = PrAt {
            repository: repository.clone(),
            number,
            head: head.clone(),
        };
        let start = start(&pr);
        let target = Against::Branch(&pr.target_branch, &start);
        let held = |open: &OpenDiff| FilesRead::Held {
            diff: shown(open),
            threads: (content.as_ref())
                .filter(|content| content.head.as_ref() == Some(&open.pr.head))
                .map(|content| threads::place(&open.diff, content.threads()))
                .unwrap_or_default(),
        };
        // The diff on show is in the mirror, so asking git again would only cost a moment.
        if let Some(open) = self.diffs.shown(&at, target) {
            return Ok(Some(held(&open)));
        }
        match diff::held(&mirror, &pr.target_branch, &head) {
            Held::NoHead => {
                let older = diff::last_head(&mirror, number).filter(|older| {
                    matches!(diff::held(&mirror, &pr.target_branch, older), Held::Both)
                });
                let older = older.map(|older| {
                    let read = || diff::read(&mirror, &pr.target_branch, &start, &older);
                    let at = PrAt {
                        head: older.clone(),
                        ..at
                    };
                    let open = self.diffs.get_or_read(&at, target, read)?;
                    let count = (content.as_ref()).map_or(0, |content| content.threads().count());
                    Ok::<_, Error>(OlderDiff {
                        diff: shown(&open),
                        threads: u32::try_from(count).unwrap_or(u32::MAX),
                    })
                });
                let older = older.transpose()?;
                let head = PrHead {
                    number,
                    commit: head,
                    target_branch: pr.target_branch,
                };
                Ok(Some(FilesRead::Missing { head, older }))
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
                let open = self.diffs.get_or_read(&at, target, read)?;
                Ok(Some(held(&open)))
            }
        }
    }

    /// What PR `number`'s Mirror holds of its head, as the user opens the PR, or nothing if Tsugiki doesn't
    /// hold the PR or its mirror isn't there. It runs one git, which says whether the mirror holds the head,
    /// so a PR left on its Conversation costs no diff.
    pub fn head_in_mirror(
        &self,
        repository: &RepositoryName,
        number: u32,
    ) -> Result<Option<HeadInMirror>, Error> {
        let Some((pr, _content)) = self.reader.pull_request(repository, number)? else {
            return Ok(None);
        };
        let mirror = self.folders.mirror(repository);
        if !mirror.exists() {
            return Ok(None);
        }
        let holds = |head: &CommitSha| diff::holds_commit(&mirror, head);
        Ok(Some(HeadInMirror::of(&pr, holds)))
    }

    /// The rows in `range` of the diff `of`, cut to its end. They come from the diff on show, unless the
    /// user has opened another PR since.
    pub fn rows(&self, of: &DiffOf, range: Range<u32>) -> Result<Vec<DiffRow>, Error> {
        Ok(self.open(of)?.diff.rows(range).to_vec())
    }

    /// The row of the diff `of` that shows `line`, or else its line at the head, which a gap or the end of
    /// its file hides. Go to definition lands there (#444, #445). A file the diff has no such line in has
    /// neither.
    pub fn row(&self, of: &DiffOf, line: &FileLine) -> Result<Option<LineInDiff>, Error> {
        let open = self.open(of)?;
        let diff = &open.diff;
        let files = diff.files();
        let (side, named): (Side, fn(&DiffFile) -> &str) = match line.side {
            DiffEnd::Base => (Side::Left, |file| {
                file.old_path.as_deref().unwrap_or(&file.path)
            }),
            DiffEnd::Head => (Side::Right, |file| &file.path),
        };
        // A PR can rename a file and add another by its old name, which has no line at the base.
        let found = ((0_u32..).zip(files))
            .filter(|(_, file)| named(file) == line.path)
            .find_map(|(index, file)| {
                let rows = diff.rows_of(file);
                if let Some(row) = threads::row_of(diff, rows.clone(), side, line.line) {
                    return Some(LineInDiff::Row { row });
                }
                let (_, _, head_line) = threads::outside(file, rows.end, side, line.line)?;
                Some(LineInDiff::Hidden {
                    file: index,
                    head_line,
                })
            });
        Ok(found)
    }

    /// Where the code of the rows of the diff `of` holds `query`, whatever the case: in every row, those the
    /// window hasn't asked for too (#186).
    pub fn find(&self, of: &DiffOf, query: &str) -> Result<Found, Error> {
        Ok(self.open(of)?.diff.find(query))
    }

    /// The spans of the rows in `range`, which highlight their code (#178). Those of a file the window shows
    /// collapsed come only once it opens the file, and `collapsed` names those, by their place among the
    /// files (#181).
    pub fn spans(
        &self,
        of: &DiffOf,
        range: Range<u32>,
        collapsed: &[u32],
    ) -> Result<Vec<Vec<Span>>, Error> {
        let mirror = self.folders.mirror(&of.pr.repository);
        let open = self.open(of)?;
        let chunk = Chunk {
            rows: range,
            collapsed,
        };
        Ok(self.highlighter.spans(&open, &mirror, chunk))
    }

    /// Lines `lines` of file `file` at the PR's head, counted from 1, with their spans: those a gap of the
    /// diff `of` leaves out, which the user expands (#182). They come from the mirror, so expanding asks
    /// GitHub for nothing. Those past the file's end don't come.
    pub fn lines(&self, of: &DiffOf, file: u32, lines: Range<u32>) -> Result<Vec<HeadLine>, Error> {
        let mirror = self.folders.mirror(&of.pr.repository);
        let open = self.open(of)?;
        let file = usize::try_from(file).unwrap_or(usize::MAX);
        let wanted = FileLines { file, lines };
        Ok(self.highlighter.head_lines(&open, &mirror, wanted))
    }

    /// The diff `of`: the one on show, or else read again from its base, as when the user has opened
    /// another PR since. The Target branch only says which files are Generated.
    fn open(&self, of: &DiffOf) -> Result<Arc<OpenDiff>, Error> {
        let DiffOf { pr, base } = of;
        self.diffs.get_or_read(pr, Against::Base(base), || {
            let facts = self.reader.pull_request(&pr.repository, pr.number)?;
            let Some((facts, _content)) = facts else {
                return Err(Error::new(
                    ErrorKind::NotFound,
                    &format!("Tsugiki doesn't hold PR #{}.", pr.number),
                ));
            };
            let mirror = self.folders.mirror(&pr.repository);
            diff::read(
                &mirror,
                &facts.target_branch,
                &Start::Base(base.clone()),
                &pr.head,
            )
        })
    }
}

/// What the Files tab shows of the diff `open`.
fn shown(open: &OpenDiff) -> ShownDiff {
    ShownDiff {
        head: open.pr.head.clone(),
        base: open.base.clone(),
        files: open.diff.files().to_vec(),
        rows: open.diff.row_count(),
        news: open.news.clone(),
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

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
