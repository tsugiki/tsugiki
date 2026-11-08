//! A PR's diff, read from its Repository's mirror (ADR 0002): `git diff` from the merge base with its Target
//! branch, or for a merged PR with the commit GitHub compared it with, to its head, through the git CLI
//! (ADR 0003). The rows stay in the core, and the window asks for them in chunks, so it holds only those
//! near what it shows (ADR 0001).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::error::{Error, ErrorKind};
use crate::github::{CommitSha, live};
use crate::repository::RepositoryName;

pub use highlighter::{Chunk, FileLines, HeadLine, Highlighter};
pub use news::{LinkTo, NewsBlock, NewsEntry, NewsLink, NewsPiece};

/// A file the diff changes, as the Files tab lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct DiffFile {
    pub path: String,
    /// Where a renamed file was.
    pub old_path: Option<String>,
    pub change: FileChange,
    /// Its header's row. Its own rows follow, up to the next file's header.
    pub row: u32,
    /// How many characters its longest line has, so the window can make room for it before it scrolls
    /// there.
    pub columns: u32,
    /// Whether `.gitattributes` on the Target branch marks it `linguist-generated`, which the Files tab
    /// starts collapsed.
    pub generated: bool,
    /// The lines its hunks leave out above each of them, which the window can show (#182).
    pub gaps: Vec<Gap>,
    /// Where the lines after its last hunk start, if the file may go on there.
    pub rest: Option<Rest>,
    /// Its changes, each a run of removed rows and the added rows after them, which the side-by-side view
    /// shows beside each other (#183).
    pub pairs: Vec<Pair>,
}

/// Removed rows and the added rows right after them: the lines a change replaced, and what replaced them.
/// Either side may have no rows. A side's rows end with the note that its last line has no newline, if git
/// wrote one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Pair {
    /// The pair's first row. The added rows start after the removed ones.
    pub row: u32,
    /// How many rows the removed side has.
    pub removed: u32,
    /// How many rows the added side has.
    pub added: u32,
}

/// The lines of a file that a diff leaves out above one of its hunks: the same lines at the merge base and
/// at the head, so either number counts on from the gap's first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Gap {
    /// The row of the hunk below the gap.
    pub row: u32,
    /// The gap's first line at the merge base.
    pub old: u32,
    /// The gap's first line at the head.
    pub new: u32,
    /// How many lines it leaves out.
    pub lines: u32,
}

/// The first line after a file's last hunk. git ends a hunk with 3 lines of context where the file goes
/// on, so a hunk that ends with fewer, or with the file's missing newline, ends the file. How many lines
/// follow only the file at the head says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Rest {
    pub old: u32,
    pub new: u32,
}

/// The lines of context git puts around each change, which `--unified=3` asks for.
const CONTEXT: usize = 3;

/// How much a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileChange {
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
    /// A file's header, as the list of files has it.
    File {
        path: String,
        old_path: Option<String>,
        change: FileChange,
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
        for (file, section) in files.iter_mut().zip(sections) {
            file.row = count(rows.len());
            rows.push(DiffRow::File {
                path: file.path.clone(),
                old_path: file.old_path.clone(),
                change: file.change,
            });
            let first = rows.len();
            (file.gaps, file.rest) = parse_section(section, &mut rows)?;
            file.columns = rows.get(first..).map_or(0, widest);
            file.pairs = rows
                .get(first..)
                .map_or_else(Vec::new, |own| pairs(own, first));
        }
        Ok(Self { files, rows })
    }

    /// Marks the files `generated` says are, one answer a file, and no others. Without answers it marks
    /// none, and says why in the log.
    fn mark_generated(&mut self, generated: std::io::Result<Vec<bool>>, mirror: &Path) {
        let generated = generated.unwrap_or_else(|cause| {
            let mirror = mirror.display();
            log::warn!(
                "Tsugiki can't tell which files of a diff in {mirror} are generated, so it shows them all: \
                 {cause}"
            );
            Vec::new()
        });
        for (at, file) in self.files.iter_mut().enumerate() {
            file.generated = generated.get(at).copied().unwrap_or(false);
        }
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
            (Ok(added), Ok(removed)) => FileChange::Lines { added, removed },
            _ if (added, removed) == ("-", "-") => FileChange::Binary,
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
            columns: 0,
            generated: false,
            gaps: Vec::new(),
            rest: None,
            pairs: Vec::new(),
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

/// The rows of one file's section: notes from its header, then its hunks. It gives the lines the hunks leave
/// out above each, and where the lines after the last start, if the file goes on there.
fn parse_section(
    section: Vec<&str>,
    rows: &mut Vec<DiffRow>,
) -> Result<(Vec<Gap>, Option<Rest>), Error> {
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
    let mut gaps = Vec::new();
    // The first line after the hunks so far, on each side, and the rows of context the last ends with.
    let (mut old_end, mut new_end, mut trailing) = (1, 1, 0);
    let mut hunks = false;
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
                let lines = new.saturating_sub(new_end);
                if lines > 0 {
                    let (old, new, row) = (old_end, new_end, count(rows.len()));
                    gaps.push(Gap {
                        row,
                        old,
                        new,
                        lines,
                    });
                }
                hunks = true;
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
        trailing = if matches!(row, DiffRow::Context { .. }) {
            trailing + 1
        } else {
            0
        };
        (old_end, new_end) = (old, new);
        rows.push(row);
    }
    let rest = (hunks && trailing >= CONTEXT).then_some(Rest {
        old: old_end,
        new: new_end,
    });
    Ok((gaps, rest))
}

/// The changes of `rows`, where `rows` starts at row `first` of the diff.
fn pairs(rows: &[DiffRow], first: usize) -> Vec<Pair> {
    let mut pairs = Vec::new();
    let mut at = 0;
    while at < rows.len() {
        let removed = side(rows, at, |row| matches!(row, DiffRow::Removed { .. }));
        let added = side(rows, at + removed, |row| {
            matches!(row, DiffRow::Added { .. })
        });
        if removed + added == 0 {
            at += 1;
            continue;
        }
        pairs.push(Pair {
            row: count(first + at),
            removed: count(removed),
            added: count(added),
        });
        at += removed + added;
    }
    pairs
}

/// How many rows from `from` on are lines that `changed` picks, with the notes right after them.
fn side(rows: &[DiffRow], from: usize, changed: fn(&DiffRow) -> bool) -> usize {
    let rows = rows.get(from..).unwrap_or_default();
    let lines = rows.iter().take_while(|row| changed(row)).count();
    let rest = rows.get(lines..).unwrap_or_default();
    let notes = rest
        .iter()
        .take_while(|row| matches!(row, DiffRow::Note { .. }))
        .count();
    if lines == 0 { 0 } else { lines + notes }
}

/// How many characters the longest line of `rows` has.
fn widest(rows: &[DiffRow]) -> u32 {
    let line = |row: &DiffRow| match row {
        DiffRow::Context { text, .. }
        | DiffRow::Added { text, .. }
        | DiffRow::Removed { text, .. } => text.chars().count(),
        DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => 0,
    };
    count(rows.iter().map(line).max().unwrap_or(0))
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

/// The commit `mirror` last fetched as PR `number`'s head, if it holds one. It stays there when the PR moves
/// on, until the mirror fetches the newer head.
pub fn last_head(mirror: &Path, number: u32) -> Option<CommitSha> {
    let named = format!("refs/tsugiki/pull/{number}/head^{{commit}}");
    let found = git(mirror, &["rev-parse", "--verify", "--quiet", &named]).ok()?;
    CommitSha::parse(found.trim())
}

/// Where a PR's diff starts: at the merge base of its head and another commit, or at a commit the window
/// holds.
#[derive(Debug, Clone, Copy)]
pub enum Start<'a> {
    /// The merge base with the Target branch, as for an open PR.
    TargetBranch,
    /// The merge base with this commit, the one GitHub compared a merged PR with. The PR's head can be in
    /// the Target branch by then, which would leave nothing to show (#212).
    MergeBaseWith(&'a CommitSha),
    /// The merge base with the first parent of this merge commit, for a merged PR stored before Tsugiki
    /// read the commit GitHub compared it with.
    MergeBaseWithParentOf(&'a CommitSha),
    /// This commit. A chunk of a diff the window holds comes from the commits it was read between,
    /// whatever the Target branch is now (#211).
    Base(&'a CommitSha),
}

/// Reads the diff from `start` to `head`, with renames found as GitHub finds them, the files the branch
/// `target` marks generated, and the NEWS entries at `head`. The head and the branch have to be in
/// `mirror`, as `held` says.
pub fn read(
    mirror: &Path,
    target: &str,
    start: Start<'_>,
    head: &CommitSha,
) -> Result<MirrorDiff, Error> {
    let index = TargetIndex::new(mirror);
    // The index takes as long as a small diff, so the two run side by side.
    let (diff, made) = std::thread::scope(|scope| {
        let made = scope.spawn(|| index.make(target));
        (run_diff(mirror, target, start, head), made.join())
    });
    let (base, numstat, patch) = diff.map_err(|cause| cant_read(mirror, &cause))?;
    let mut diff = Diff::parse(&numstat, &patch)?;
    let stopped = std::io::Error::other("the index's reader stopped");
    let made = made.unwrap_or(Err(stopped));
    // Both need the list of files, and each starts a git.
    let (generated, news) = std::thread::scope(|scope| {
        let news = scope.spawn(|| news(mirror, head, diff.files()));
        let generated = made.and_then(|()| index.generated(diff.files()));
        (generated, news.join().unwrap_or_default())
    });
    diff.mark_generated(generated, mirror);
    Ok(MirrorDiff { base, diff, news })
}

/// A PR's diff as read from its mirror, the merge base it starts from, and its NEWS entries.
#[derive(Debug)]
pub struct MirrorDiff {
    /// The commit whose versions of the files the removed lines come from.
    pub base: CommitSha,
    pub diff: Diff,
    pub news: Vec<NewsEntry>,
}

/// An index of the Target branch's files, made for a moment so `git check-attr` can read the
/// `.gitattributes` there. git reads them from a commit only since 2.40 (`--source`), and Tsugiki asks
/// for 2.30. It lives in the mirror's folder, and goes when this does. One a run left when it stopped
/// short goes with the next read.
struct TargetIndex<'a> {
    mirror: &'a Path,
    path: PathBuf,
}

impl<'a> TargetIndex<'a> {
    /// An index of a name no other is using, as reads run side by side. Only one Tsugiki runs at a time,
    /// so the indexes another run named are left over, and this removes them.
    fn new(mirror: &'a Path) -> Self {
        static MADE: AtomicU64 = AtomicU64::new(0);
        let made = MADE.fetch_add(1, Ordering::Relaxed);
        let run = format!("{INDEX}{}-", std::process::id());
        for entry in std::fs::read_dir(mirror).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(INDEX) && !name.starts_with(&run) {
                remove(&entry.path());
            }
        }
        Self {
            mirror,
            path: mirror.join(format!("{run}{made}.index")),
        }
    }

    /// Fills the index from the branch `target`.
    fn make(&self, target: &str) -> std::io::Result<()> {
        let tree = format!("refs/heads/{target}");
        self.git(&["read-tree", &tree], Vec::new()).map(drop)
    }

    /// Whether each of `files` is marked `linguist-generated`, set or `true`, as `make` left the index.
    fn generated(&self, files: &[DiffFile]) -> std::io::Result<Vec<bool>> {
        let input: Vec<u8> = files
            .iter()
            .flat_map(|file| file.path.bytes().chain([0]))
            .collect();
        let args = [
            "check-attr",
            "--cached",
            "--stdin",
            "-z",
            "linguist-generated",
        ];
        let output = self.git(&args, input)?;
        let output = String::from_utf8_lossy(&output);
        // Each answer is the path, the attribute and its value, each ended by a NUL.
        let fields: Vec<&str> = output.split('\0').collect();
        let marked: HashMap<&str, bool> = fields
            .as_chunks::<3>()
            .0
            .iter()
            .map(|[path, _, value]| (*path, matches!(*value, "set" | "true")))
            .collect();
        Ok(files
            .iter()
            .map(|file| marked.get(file.path.as_str()).copied().unwrap_or(false))
            .collect())
    }

    /// A git in the mirror with `args`, which reads and writes this index. It reads only the attributes
    /// the index holds: not the user's own file, nor the computer's.
    fn git(&self, args: &[&str], input: Vec<u8>) -> std::io::Result<Vec<u8>> {
        let mut all = ["-C", "", "-c", "core.attributesFile="].map(OsStr::new);
        all[1] = self.mirror.as_os_str();
        let all: Vec<&OsStr> = all.into_iter().chain(args.iter().map(OsStr::new)).collect();
        let envs = [
            ("GIT_INDEX_FILE", self.path.as_os_str()),
            ("GIT_ATTR_NOSYSTEM", OsStr::new("1")),
        ];
        live::read_git_bytes_with_env(&all, input, &envs)
    }
}

/// How the name of each index of a Target branch starts.
const INDEX: &str = "tsugiki-attributes-";

impl Drop for TargetIndex<'_> {
    fn drop(&mut self) {
        remove(&self.path);
    }
}

/// Removes the file at `path`, if it's there.
fn remove(path: &Path) {
    match std::fs::remove_file(path) {
        Err(cause) if cause.kind() != std::io::ErrorKind::NotFound => {
            let path = path.display();
            log::warn!("Tsugiki can't remove {path}: {cause}");
        }
        _ => {}
    }
}

/// The NEWS entries among `files`, rendered from `head`. Those git can't read stay out.
fn news(mirror: &Path, head: &CommitSha, files: &[DiffFile]) -> Vec<NewsEntry> {
    // git reads a name a line.
    let paths: Vec<&str> = files
        .iter()
        .map(|file| file.path.as_str())
        .filter(|path| NewsEntry::is_entry(path) && !path.contains('\n'))
        .collect();
    let names: Vec<String> = paths
        .iter()
        .map(|path| format!("{}:{path}", head.as_str()))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    highlighter::blobs(mirror, &names)
        .into_iter()
        .zip(paths)
        .filter_map(|(text, path)| Some(NewsEntry::render(path, &text?)))
        .collect()
}

/// The merge base `read` starts from, and what `git diff --numstat -z` and `git diff` print from it.
fn run_diff(
    mirror: &Path,
    target: &str,
    start: Start<'_>,
    head: &CommitSha,
) -> std::io::Result<(CommitSha, String, String)> {
    let base_commit = match start {
        Start::TargetBranch => merge_base(mirror, &format!("refs/heads/{target}"), head)?,
        Start::MergeBaseWith(commit) => merge_base(mirror, commit.as_str(), head)?,
        Start::MergeBaseWithParentOf(merge) => {
            merge_base(mirror, &format!("{}^1", merge.as_str()), head)?
        }
        Start::Base(base) => base.clone(),
    };
    let base = base_commit.as_str();
    let head = head.as_str();
    // A user's git config can change how a diff looks, so these say each choice outright.
    let options = [
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-relative",
        "--find-renames",
        "--unified=3",
        // `diff.submodule=log` prints a submodule without its `diff --git` line, which the list still has.
        "--submodule=short",
    ];
    let numstat = [
        ["diff", "--numstat", "-z"].as_slice(),
        &options,
        &[base, head],
    ]
    .concat();
    let patch = [["diff"].as_slice(), &options, &[base, head]].concat();
    // The two run side by side: a diff of 800 files takes each most of a second.
    let (numstat, patch) = std::thread::scope(|scope| {
        let numstat = scope.spawn(|| git(mirror, &numstat));
        (numstat.join(), git(mirror, &patch))
    });
    let stopped = std::io::Error::other("git's reader stopped");
    Ok((base_commit, numstat.unwrap_or(Err(stopped))?, patch?))
}

/// The merge base of `head` and the commit git's revision `other` names.
fn merge_base(mirror: &Path, other: &str, head: &CommitSha) -> std::io::Result<CommitSha> {
    let base = git(mirror, &["merge-base", other, head.as_str()])?;
    let base = base.trim();
    // Built every time, as a closure that never runs would fail the coverage gate (ADR 0009).
    let not_a_commit = std::io::Error::other(format!("merge-base gave {base:?}"));
    CommitSha::parse(base).ok_or(not_a_commit)
}

fn git(mirror: &Path, args: &[&str]) -> std::io::Result<String> {
    let mut all = vec![OsStr::new("-C"), mirror.as_os_str()];
    all.extend(args.iter().map(OsStr::new));
    live::read_git(&all)
}

fn cant_read(mirror: &Path, cause: &std::io::Error) -> Error {
    let mirror = mirror.display();
    log::warn!("Tsugiki can't read a diff from {mirror}: {cause}");
    Error::new(
        ErrorKind::Internal,
        "Tsugiki couldn't read this PR's diff from its mirror.",
    )
}

/// A PR at one head. Its diff starts where the head leaves its Target branch, so a PR moved to another
/// branch has another diff at the same head.
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

/// A PR's diff as the window holds it: from the merge base it was told to the PR's head. Each chunk names
/// both, so its rows come from the commits the window's list of files does (#211).
#[derive(Debug, Clone)]
pub struct DiffOf {
    pub pr: PrAt,
    pub base: CommitSha,
}

/// Which of a PR's diffs at one head a caller wants.
#[derive(Debug, Clone, Copy)]
pub enum Against<'a> {
    /// The diff against the branch the PR targets now, by its name.
    Branch(&'a str),
    /// The diff from a merge base the window holds.
    Base(&'a CommitSha),
}

impl Against<'_> {
    /// The branch a diff read for this is read against, if this names one.
    fn branch(self) -> Option<String> {
        match self {
            Self::Branch(target) => Some(target.to_owned()),
            Self::Base(_) => None,
        }
    }
}

/// A diff the window reads in chunks, and the PR it belongs to.
#[derive(Debug)]
pub struct OpenDiff {
    pub pr: PrAt,
    /// The Target branch it was read against. A diff read from a merge base the window held has none,
    /// whatever the PR targets now: that branch may have another merge base.
    pub target: Option<String>,
    pub base: CommitSha,
    pub diff: Diff,
    pub news: Vec<NewsEntry>,
}

impl OpenDiff {
    /// Whether this is the diff of `pr` that `against` asks for.
    fn answers(&self, pr: &PrAt, against: Against<'_>) -> bool {
        self.pr.is(pr)
            && match against {
                Against::Branch(target) => self.target.as_deref() == Some(target),
                Against::Base(base) => self.base == *base,
            }
    }
}

/// The diff of the PR on show, which each chunk the window asks for comes from. Commands run on threads of
/// their own, so the lock is held only to look or to swap, and never while git runs.
#[derive(Debug, Default)]
pub struct Diffs {
    shown: Mutex<Shown>,
}

/// The diff on show, and the read that put it there.
#[derive(Debug, Default)]
struct Shown {
    /// How many reads have started.
    reads: u64,
    /// Which of them put `open` there.
    read: u64,
    open: Option<Arc<OpenDiff>>,
}

impl Diffs {
    /// The diff of `pr` that `against` asks for, if it is the one on show.
    pub fn shown(&self, pr: &PrAt, against: Against<'_>) -> Option<Arc<OpenDiff>> {
        let open = self.lock().open.clone();
        open.filter(|open| open.answers(pr, against))
    }

    /// The diff of `pr` that `against` asks for: the one on show, or else what `read` gives, which takes
    /// its place unless a read that started later got there first. That one is of the PR the user opened
    /// since.
    pub fn get_or_read(
        &self,
        pr: &PrAt,
        against: Against<'_>,
        read: impl FnOnce() -> Result<MirrorDiff, Error>,
    ) -> Result<Arc<OpenDiff>, Error> {
        let started = {
            let mut shown = self.lock();
            if let Some(open) = shown.open.clone().filter(|open| open.answers(pr, against)) {
                return Ok(open);
            }
            shown.reads += 1;
            shown.reads
        };
        let MirrorDiff { base, diff, news } = read()?;
        let read = Arc::new(OpenDiff {
            pr: pr.clone(),
            target: against.branch(),
            base,
            diff,
            news,
        });
        let mut shown = self.lock();
        if started > shown.read {
            shown.read = started;
            shown.open = Some(Arc::clone(&read));
        }
        drop(shown);
        Ok(read)
    }

    /// A panic while the lock was held left nothing half done: the lock only guards a look or a swap.
    fn lock(&self) -> std::sync::MutexGuard<'_, Shown> {
        self.shown.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub use find::{Found, MOST_MATCHES, Match};

mod find;
mod highlighter;
mod news;

#[cfg(test)]
pub(crate) mod tests;
