//! A PR's diff, read from its Repository's mirror (ADR 0002): `git diff` from the merge base with its Target
//! branch to its head, through the git CLI (ADR 0003). The rows stay in the core, and the window asks for
//! them in chunks, so it holds only those near what it shows (ADR 0001).

use std::ffi::OsStr;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::github::{CommitSha, live};
use crate::repository::RepositoryName;

/// A file the diff changes, as the Files tab lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DiffFile {
    pub path: String,
    /// Where a renamed file was.
    pub old_path: Option<String>,
    pub change: Change,
    /// Its header's row. Its own rows follow, up to the next file's header.
    pub row: u32,
}

/// How much a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    Lines {
        added: u32,
        removed: u32,
    },
    /// git doesn't count the lines of a binary file.
    Binary,
}

/// One row of the diff, as the window draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffRow {
    /// A file's header, naming the file by where it is in the list of files.
    File {
        file: u32,
    },
    /// A hunk's `@@` line, with the function git found around it.
    Hunk {
        text: String,
    },
    Context {
        old: u32,
        new: u32,
        text: String,
    },
    Added {
        new: u32,
        text: String,
    },
    Removed {
        old: u32,
        text: String,
    },
    /// What git says in place of lines: a binary file, a new mode, a missing newline at the end.
    Note {
        text: String,
    },
}

/// A PR's diff: its files, and every row of them in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    files: Vec<DiffFile>,
    rows: Vec<DiffRow>,
}

impl Diff {
    pub fn files(&self) -> &[DiffFile] {
        &self.files
    }

    /// How many rows the diff has, file headers included.
    pub fn row_count(&self) -> u32 {
        count(self.rows.len())
    }

    /// The rows in `range`, cut to the diff's end.
    pub fn rows(&self, range: Range<u32>) -> &[DiffRow] {
        let at = |row: u32| usize::try_from(row).unwrap_or(usize::MAX);
        let end = at(range.end).min(self.rows.len());
        self.rows.get(at(range.start).min(end)..end).unwrap_or(&[])
    }

    /// Reads `git diff --numstat -z` and the `git diff` of the same commits. The first names each file
    /// exactly, where the patch quotes odd names, and the two list the files in the same order.
    pub fn parse(numstat: &str, patch: &str) -> Result<Self, Error> {
        let mut files = parse_numstat(numstat)?;
        let mut rows = Vec::new();
        let sections = sections(patch);
        if sections.len() != files.len() {
            return Err(unreadable(&format!(
                "the patch has {} files, and the list {}",
                sections.len(),
                files.len()
            )));
        }
        for (index, (file, section)) in files.iter_mut().zip(sections).enumerate() {
            file.row = count(rows.len());
            rows.push(DiffRow::File { file: count(index) });
            parse_section(section, &mut rows)?;
        }
        Ok(Self { files, rows })
    }
}

/// A count of rows or files. A diff never comes near `u32::MAX` of either.
fn count(length: usize) -> u32 {
    u32::try_from(length).unwrap_or(u32::MAX)
}

fn unreadable(why: &str) -> Error {
    log::warn!("Tsugiki can't read git's diff: {why}");
    Error::new(ErrorKind::Internal, "Tsugiki couldn't read git's diff.")
}

/// Each file of `git diff --numstat -z`. A file's entry is its counts and its path, ended by a NUL. A
/// renamed file's path is empty, and its old and new paths follow, each ended by a NUL.
fn parse_numstat(numstat: &str) -> Result<Vec<DiffFile>, Error> {
    let mut files = Vec::new();
    let mut fields = numstat.split('\0');
    while let Some(entry) = fields.next().filter(|entry| !entry.is_empty()) {
        let mut parts = entry.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(unreadable(&format!("a file's counts read {entry:?}")));
        };
        let change = match (added.parse(), removed.parse()) {
            (Ok(added), Ok(removed)) => Change::Lines { added, removed },
            _ if (added, removed) == ("-", "-") => Change::Binary,
            _ => return Err(unreadable(&format!("a file's counts read {entry:?}"))),
        };
        let (path, old_path) = if path.is_empty() {
            let mut path = || fields.next().filter(|path| !path.is_empty());
            let (Some(old), Some(new)) = (path(), path()) else {
                return Err(unreadable("a renamed file has no paths"));
            };
            (new.to_owned(), Some(old.to_owned()))
        } else {
            (path.to_owned(), None)
        };
        files.push(DiffFile {
            path,
            old_path,
            change,
            row: 0,
        });
    }
    Ok(files)
}

/// The patch cut into one section per file, each starting at its `diff --git` line.
fn sections(patch: &str) -> Vec<Vec<&str>> {
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for line in patch.lines() {
        match sections.last_mut() {
            Some(section) if !line.starts_with("diff --git ") => section.push(line),
            _ => sections.push(vec![line]),
        }
    }
    sections
}

/// The rows of one file's section: notes from its header, then its hunks.
fn parse_section(section: Vec<&str>, rows: &mut Vec<DiffRow>) -> Result<(), Error> {
    let mut lines = section.into_iter().skip(1).peekable();
    let mut old_mode = None;
    while let Some(line) = lines.next_if(|line| !line.starts_with("@@")) {
        if let Some(mode) = line.strip_prefix("old mode ") {
            old_mode = Some(mode);
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            let from = old_mode.unwrap_or("?");
            note(rows, &format!("File mode changed from {from} to {mode}"));
        } else if line.starts_with("Binary files ") {
            note(rows, "Binary file not shown");
        }
    }
    let (mut old, mut new) = (0, 0);
    for line in lines {
        let mut chars = line.chars();
        let marker = chars.next();
        let text = chars.as_str().to_owned();
        let row = match marker {
            // `diff.suppressBlankEmpty` drops the space before an empty line of context.
            Some(' ') | None => DiffRow::Context { old, new, text },
            Some('+') => DiffRow::Added { new, text },
            Some('-') => DiffRow::Removed { old, text },
            Some('\\') => DiffRow::Note {
                text: "No newline at end of file".to_owned(),
            },
            Some(_) => {
                (old, new) = hunk_starts(line)?;
                rows.push(DiffRow::Hunk {
                    text: line.to_owned(),
                });
                continue;
            }
        };
        match &row {
            DiffRow::Context { .. } => (old, new) = (old + 1, new + 1),
            DiffRow::Added { .. } => new += 1,
            DiffRow::Removed { .. } => old += 1,
            DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => {}
        }
        rows.push(row);
    }
    Ok(())
}

fn note(rows: &mut Vec<DiffRow>, text: &str) {
    rows.push(DiffRow::Note {
        text: text.to_owned(),
    });
}

/// The first old and new line numbers of a hunk, from its header `@@ -12,7 +12,8 @@ def f():`.
fn hunk_starts(header: &str) -> Result<(u32, u32), Error> {
    let start = |side: Option<&str>, sign: char| {
        let numbers = side?.strip_prefix(sign)?;
        numbers.split(',').next()?.parse::<u32>().ok()
    };
    let mut sides = header.strip_prefix("@@ ").unwrap_or("").split(' ');
    match (start(sides.next(), '-'), start(sides.next(), '+')) {
        (Some(old), Some(new)) => Ok((old, new)),
        _ => Err(unreadable(&format!("a hunk starts {header:?}"))),
    }
}

/// What the mirror holds of a PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The PR's head isn't in the mirror yet.
    NoHead,
    /// The mirror has no branch by the PR's Target branch's name.
    NoTarget,
    Both,
}

/// Says whether `mirror` holds `head` and the branch `target`, which a diff needs both of.
pub fn held(mirror: &Path, target: &str, head: &CommitSha) -> Held {
    let has = |object: &str| git(mirror, &["cat-file", "-e", object]).is_ok();
    if !has(&format!("{}^{{commit}}", head.as_str())) {
        Held::NoHead
    } else if !has(&format!("refs/heads/{target}")) {
        Held::NoTarget
    } else {
        Held::Both
    }
}

/// Reads the diff from the merge base of `head` and the branch `target` to `head`, with renames found as
/// GitHub finds them. Both have to be in `mirror`, as `held` says.
pub fn read(mirror: &Path, target: &str, head: &CommitSha) -> Result<Diff, Error> {
    let head = head.as_str();
    let base = git(
        mirror,
        &["merge-base", &format!("refs/heads/{target}"), head],
    )
    .map_err(|cause| cant_read(mirror, &cause))?;
    let base = base.trim();
    // A user's git config can change how a diff looks, so these say each choice outright.
    let options = [
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-relative",
        "--find-renames",
        "--unified=3",
    ];
    let numstat = [
        ["diff", "--numstat", "-z"].as_slice(),
        &options,
        &[base, head],
    ]
    .concat();
    let patch = [["diff"].as_slice(), &options, &[base, head]].concat();
    let numstat = git(mirror, &numstat).map_err(|cause| cant_read(mirror, &cause))?;
    let patch = git(mirror, &patch).map_err(|cause| cant_read(mirror, &cause))?;
    Diff::parse(&numstat, &patch)
}

fn git(mirror: &Path, args: &[&str]) -> std::io::Result<String> {
    let mut all = vec![OsStr::new("-C"), mirror.as_os_str()];
    all.extend(args.iter().map(OsStr::new));
    live::read_git(&all)
}

fn cant_read(mirror: &Path, cause: &std::io::Error) -> Error {
    log::warn!(
        "Tsugiki can't read a diff from {}: {cause}",
        mirror.display()
    );
    Error::new(
        ErrorKind::Internal,
        "Tsugiki couldn't read this PR's diff from its mirror.",
    )
}

/// A PR at one head, which has one diff.
#[derive(Debug, Clone)]
pub struct PrAt {
    pub repository: RepositoryName,
    pub number: u32,
    pub head: CommitSha,
}

impl PrAt {
    fn is(&self, other: &Self) -> bool {
        self.repository.is_same(&other.repository)
            && self.number == other.number
            && self.head == other.head
    }
}

/// A diff the window reads in chunks, and the PR it belongs to.
#[derive(Debug)]
pub struct OpenDiff {
    pub pr: PrAt,
    pub diff: Diff,
}

/// The diff of the PR on show, which each chunk the window asks for comes from. Commands run on threads of
/// their own, so the lock is held only to look or to swap, and never while git runs.
#[derive(Debug, Default)]
pub struct Diffs {
    open: Mutex<Option<Arc<OpenDiff>>>,
}

impl Diffs {
    /// The diff of `pr`: the one on show, or else what `read` gives, which takes its place.
    pub fn get_or_read(
        &self,
        pr: &PrAt,
        read: impl FnOnce() -> Result<Diff, Error>,
    ) -> Result<Arc<OpenDiff>, Error> {
        let open = self.lock().clone();
        if let Some(open) = open.filter(|open| open.pr.is(pr)) {
            return Ok(open);
        }
        let read = Arc::new(OpenDiff {
            pr: pr.clone(),
            diff: read()?,
        });
        *self.lock() = Some(Arc::clone(&read));
        Ok(read)
    }

    /// A panic while the lock was held left nothing half done: the lock only guards a swap.
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Arc<OpenDiff>>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
pub(crate) mod tests;
