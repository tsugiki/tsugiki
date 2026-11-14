//! The highlighter (#178, #180): the thread that reads the versions of the open PR's files from its mirror,
//! parses them and keeps their trees, and the spans each chunk of rows reads from those trees.
//!
//! Each line a row shows is in one version of its file: a removed line in the merge base's, and the others
//! in the head's. A chunk takes the trees it needs from the shelf, where the thread puts each one it parses,
//! and asks the thread for those it lacks, which go ahead of the rest. Between chunks the thread parses the
//! rest of the PR's versions in the diff's order, NEWS entries first, so that a jump finds its file parsed.
//!
//! The thread keeps the trees of the open PR and of the one before it, which `Alt+←` goes back to, up to
//! `CAP` bytes. At the cap it stops parsing ahead, and a chunk that needs another tree drops the trees used
//! longest ago.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::OsStr;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use serde::Serialize;
use specta::Type;

use super::{DiffRow, NewsEntry, OpenDiff, count};
use crate::github::{CommitSha, live};
use crate::highlight::{Language, Parsed, Span};

/// Which version of a file a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Side {
    Base,
    Head,
}

/// One version of one of the diff's files: its place in the diff's list of files, and which side.
type Version = (usize, Side);

/// A version as parsed: its tree, or nothing if Tsugiki doesn't highlight its language or git has no such
/// file.
type Tree = Option<Arc<Parsed>>;

/// The most lines between two of a chunk's lines that are still highlighted in one read. Hunks further
/// apart are read apart, so a chunk never pays for the code between two distant hunks.
const GAP: u32 = 32;

/// The memory the kept trees may take, as `Parsed::cost` counts it: about 16 of CPython's largest C files
/// at 12 MB each, or a few hundred files of a usual size.
const CAP: usize = 200_000_000;

/// The most versions one git reads for the parse ahead. One git per version would spend more time starting
/// than reading, and a chunk that asks waits for the git under way.
const BATCH: usize = 16;

/// The PRs whose trees the thread keeps: the open one, and the one before it.
const PRS: usize = 2;

/// `lines`, which are in order, as runs to highlight, each with no gap of more than `GAP` lines.
fn runs(lines: &[u32]) -> Vec<Range<u32>> {
    let mut runs: Vec<Range<u32>> = Vec::new();
    for &line in lines {
        match runs.last_mut() {
            Some(run) if line <= run.end.saturating_add(GAP) => run.end = run.end.max(line + 1),
            _ => runs.push(line..line + 1),
        }
    }
    runs
}

/// Starts the shelf: the thread that parses gets the writer, which can't be cloned, and each chunk reads.
fn shelf() -> (ShelfWriter, ShelfReader) {
    let shared = Arc::new(Mutex::new(Vec::new()));
    (ShelfWriter(Arc::clone(&shared)), ShelfReader(shared))
}

/// The trees the thread keeps, by diff. Each function takes the lock for one look or one change, and no
/// guard leaves these types. A panic can't leave a change half made, so both sides read through a poisoned
/// lock.
type Shelves = Vec<(Arc<OpenDiff>, HashMap<Version, Tree>)>;

/// The one side that changes the shelf.
struct ShelfWriter(Arc<Mutex<Shelves>>);

impl ShelfWriter {
    fn put(&mut self, diff: &Arc<OpenDiff>, version: Version, tree: Tree) {
        let mut shelves = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if !shelves.iter().any(|(kept, _)| kept.is(diff)) {
            shelves.push((Arc::clone(diff), HashMap::new()));
        }
        if let Some((_, trees)) = shelves.iter_mut().find(|(kept, _)| kept.is(diff)) {
            drop(trees.insert(version, tree));
        }
    }

    fn take(&mut self, diff: &OpenDiff, version: Version) {
        let mut shelves = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, trees) in shelves.iter_mut().filter(|(kept, _)| kept.is(diff)) {
            drop(trees.remove(&version));
        }
    }

    /// Drops the trees of each diff but `diffs`.
    fn keep(&mut self, diffs: &[&OpenDiff]) {
        let mut shelves = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        shelves.retain(|(kept, _)| diffs.iter().any(|diff| kept.is(diff)));
    }
}

/// A read-only view of the shelf.
#[derive(Clone)]
struct ShelfReader(Arc<Mutex<Shelves>>);

impl ShelfReader {
    /// `diff`'s `version`, if the thread has parsed it and keeps it.
    fn get(&self, diff: &OpenDiff, version: Version) -> Option<Tree> {
        let shelves = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let (_, trees) = shelves.iter().find(|(kept, _)| kept.is(diff))?;
        trees.get(&version).cloned()
    }
}

/// A diff, and the mirror its files come from.
struct Source {
    diff: Arc<OpenDiff>,
    mirror: PathBuf,
}

/// What a chunk tells the thread.
enum Job {
    /// The chunk needs `versions`, and the shelf lacks some of them: parse those ahead of the rest, and
    /// answer with the trees of those it keeps.
    Parse {
        from: Source,
        versions: Vec<Version>,
        reply: Sender<HashMap<Version, Tree>>,
    },
    /// The chunk took `versions` from the shelf.
    Used {
        from: Source,
        versions: Vec<Version>,
    },
}

/// A line of a file at the PR's head that a gap leaves out, which the window shows when the user expands
/// the gap (#182), and its spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct HeadLine {
    pub text: String,
    pub spans: Vec<Span>,
}

/// Lines of one of a diff's files at the PR's head, counted from 1: those a gap leaves out, which the
/// window asks for when the user expands it. The file is its place among the diff's files.
#[derive(Debug, Clone)]
pub struct FileLines {
    pub file: usize,
    pub lines: Range<u32>,
}

/// Rows of a diff the window asks spans for, and the files it shows collapsed to their headers, by their
/// place among the files.
#[derive(Debug, Clone)]
pub struct Chunk<'a> {
    pub rows: Range<u32>,
    pub collapsed: &'a [u32],
}

/// The thread that parses the versions of the open PR's files and keeps their trees: their one writer. Each
/// chunk asks it for the spans of its rows.
#[derive(Clone)]
pub struct Highlighter {
    jobs: Sender<Job>,
    shelf: ShelfReader,
}

impl Highlighter {
    pub fn start() -> Self {
        Self::start_with(CAP)
    }

    fn start_with(cap: usize) -> Self {
        let (writer, shelf) = shelf();
        let (jobs, incoming) = mpsc::channel();
        let worker = Worker::new(writer, cap);
        let _ = thread::spawn(move || worker.run(&incoming));
        Self { jobs, shelf }
    }

    /// The spans of each row of `chunk`, read from the versions of its files in `mirror`. A row that shows
    /// no line of code, or whose file Tsugiki doesn't highlight, has none. Nor has a row of a file the chunk
    /// names as collapsed: its rows don't show, and parsing it would hold up the files that do (#181).
    pub fn spans(&self, diff: &Arc<OpenDiff>, mirror: &Path, chunk: Chunk<'_>) -> Vec<Vec<Span>> {
        let Chunk {
            rows: range,
            collapsed,
        } = chunk;
        let first = range.start;
        // A PR can have thousands of Generated files, each collapsed.
        let collapsed: BTreeSet<u32> = collapsed.iter().copied().collect();
        let shown = |&((file, _), _): &(Version, u32)| {
            !u32::try_from(file).is_ok_and(|file| collapsed.contains(&file))
        };
        let lines: Vec<Option<(Version, u32)>> = (first..)
            .zip(diff.diff.rows(range))
            .map(|(row, shown_row)| diff.line(row, shown_row).filter(shown))
            .collect();
        // The lines each version shows in this chunk, in order.
        let mut wanted: BTreeMap<Version, Vec<u32>> = BTreeMap::new();
        for &(version, line) in lines.iter().flatten() {
            wanted.entry(version).or_default().push(line);
        }
        let from = Source {
            diff: Arc::clone(diff),
            mirror: mirror.to_owned(),
        };
        let mut trees = self.trees(from, wanted.keys().copied().collect());
        let mut spans: HashMap<(Version, u32), Vec<Span>> = HashMap::new();
        for (version, mut shown) in wanted {
            let Some(Some(parsed)) = trees.remove(&version) else {
                continue;
            };
            shown.sort_unstable();
            for run in runs(&shown) {
                let line_spans = parsed.spans(run.clone());
                spans.extend(run.zip(line_spans).map(|(line, s)| ((version, line), s)));
            }
        }
        lines
            .into_iter()
            .map(|line| {
                line.and_then(|line| spans.remove(&line))
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The lines `wanted` names, read from `mirror` and highlighted from the same tree as their file's
    /// rows. Those past the file's end don't come, nor do those of a file git can't read.
    pub fn head_lines(
        &self,
        diff: &Arc<OpenDiff>,
        mirror: &Path,
        wanted: FileLines,
    ) -> Vec<HeadLine> {
        let FileLines { file, lines } = wanted;
        let path = diff.diff.files.get(file).map(|file| file.path.as_str());
        let name = path.and_then(|path| object_name(&diff.pr.head, path));
        let text = name.and_then(|name| blobs(mirror, &[&name]).into_iter().next().flatten());
        let text = text.unwrap_or_default();
        let skip = usize::try_from(lines.start.saturating_sub(1)).unwrap_or(usize::MAX);
        let take = usize::try_from(lines.end.saturating_sub(lines.start)).unwrap_or(usize::MAX);
        let texts: Vec<&str> = text.lines().skip(skip).take(take).collect();
        if texts.is_empty() {
            return Vec::new();
        }
        let shown = lines.start..lines.start.saturating_add(count(texts.len()));
        let from = Source {
            diff: Arc::clone(diff),
            mirror: mirror.to_owned(),
        };
        let version = (file, Side::Head);
        let tree = self.trees(from, vec![version]).remove(&version).flatten();
        let mut spans = tree
            .map(|parsed| parsed.spans(shown))
            .unwrap_or_default()
            .into_iter();
        texts
            .into_iter()
            .map(|text| HeadLine {
                text: text.to_owned(),
                spans: spans.next().unwrap_or_default(),
            })
            .collect()
    }

    /// The trees of `versions`: from the shelf, or else from the thread, which parses those the shelf
    /// lacks ahead of the rest. A thread that has stopped gives none.
    fn trees(&self, from: Source, versions: Vec<Version>) -> HashMap<Version, Tree> {
        let kept: Option<HashMap<Version, Tree>> = versions
            .iter()
            .map(|&version| Some((version, self.shelf.get(&from.diff, version)?)))
            .collect();
        if let Some(kept) = kept {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "the chunk has its trees, and only the order of the next drop is lost"
            )]
            let _ = self.jobs.send(Job::Used { from, versions });
            return kept;
        }
        let (reply, answer) = mpsc::channel();
        let job = Job::Parse {
            from,
            versions,
            reply,
        };
        match self.jobs.send(job).ok().and_then(|()| answer.recv().ok()) {
            Some(trees) => trees,
            None => {
                log::warn!("Tsugiki's highlighter has stopped, so the diff shows plain");
                HashMap::new()
            }
        }
    }
}

/// A tree the thread keeps, what it costs, and when a chunk last used it.
struct Kept {
    tree: Tree,
    /// As `Parsed::cost` counts it.
    cost: usize,
    /// The number of the last chunk that used it, or 0 for a tree parsed ahead that no chunk has used.
    used: u64,
}

impl Kept {
    fn new(tree: Option<Parsed>, used: u64) -> Self {
        let cost = tree.as_ref().map_or(0, Parsed::cost);
        Self {
            tree: tree.map(Arc::new),
            cost,
            used,
        }
    }
}

/// The shelf, and what the trees on it cost.
struct Store {
    shelf: ShelfWriter,
    kept: usize,
    cap: usize,
}

/// A PR whose trees the thread keeps.
struct Opened {
    source: Source,
    trees: HashMap<Version, Kept>,
    /// The versions left to parse ahead, in order.
    ahead: VecDeque<Version>,
}

impl Opened {
    fn new(source: Source) -> Self {
        let ahead = source.diff.ahead();
        Self {
            source,
            trees: HashMap::new(),
            ahead,
        }
    }

    fn diff(&self) -> &OpenDiff {
        &self.source.diff
    }

    fn cost(&self) -> usize {
        self.trees.values().map(|kept| kept.cost).sum()
    }

    /// Those of `versions` it doesn't keep.
    fn missing(&self, versions: &[Version]) -> Vec<Version> {
        let missing = |version: &&Version| !self.trees.contains_key(version);
        versions.iter().filter(missing).copied().collect()
    }

    /// `versions` read from the mirror with one git.
    fn read(&self, versions: &[Version]) -> Vec<(Version, Read)> {
        self.source.diff.read(&self.source.mirror, versions)
    }

    /// Keeps `kept` as `version`, which it didn't keep, and puts its tree on the shelf.
    fn put(&mut self, store: &mut Store, version: Version, kept: Kept) {
        store
            .shelf
            .put(&self.source.diff, version, kept.tree.clone());
        store.kept += kept.cost;
        drop(self.trees.insert(version, kept));
    }

    /// Notes that chunk `chunk` used `versions`, and gives the trees of those it keeps.
    fn used(&mut self, versions: &[Version], chunk: u64) -> HashMap<Version, Tree> {
        let mut trees = HashMap::new();
        for version in versions {
            if let Some(kept) = self.trees.get_mut(version) {
                kept.used = chunk;
                drop(trees.insert(*version, kept.tree.clone()));
            }
        }
        trees
    }

    /// Parses the next few versions ahead. It stops when a chunk asks for a tree the shelf lacks, unless
    /// that's the one it's parsing, and leaves that version for later. A version git failed to give is left
    /// for a chunk to ask for again. It says whether there was anything to parse.
    fn parse_ahead(
        &mut self,
        store: &mut Store,
        jobs: &Receiver<Job>,
        waiting: &mut VecDeque<Job>,
    ) -> bool {
        let mut batch = Vec::new();
        while let Some(version) = self.ahead.pop_front() {
            if !self.trees.contains_key(&version) {
                batch.push(version);
            }
            if batch.len() == BATCH {
                break;
            }
        }
        if batch.is_empty() {
            return false;
        }
        let diff = Arc::clone(&self.source.diff);
        let mut reads = self.read(&batch).into_iter();
        while let Some((version, read)) = reads.next() {
            let parsed = match read {
                Read::Text(language, source) => {
                    let mut stop = || {
                        waiting.extend(jobs.try_iter());
                        held_up(waiting, &diff, version)
                    };
                    Parsed::parse_unless(language, source, &mut stop)
                }
                Read::Plain => Ok(None),
                Read::Failed => continue,
            };
            let Ok(tree) = parsed else {
                let left: Vec<Version> = std::iter::once(version)
                    .chain(reads.map(|(version, _)| version))
                    .collect();
                for version in left.into_iter().rev() {
                    self.ahead.push_front(version);
                }
                break;
            };
            self.put(store, version, Kept::new(tree, 0));
        }
        true
    }
}

/// Whether a chunk in `waiting` asks for a tree, and none of them asks for `version` of `diff`, which the
/// parse under way would give.
fn held_up(waiting: &VecDeque<Job>, diff: &OpenDiff, version: Version) -> bool {
    let asks: Vec<bool> = waiting
        .iter()
        .filter_map(|job| match job {
            Job::Parse { from, versions, .. } => {
                Some(from.diff.is(diff) && versions.contains(&version))
            }
            Job::Used { .. } => None,
        })
        .collect();
    !asks.is_empty() && !asks.contains(&true)
}

/// What the thread owns: the shelf, and what it knows of each tree on it.
struct Worker {
    store: Store,
    /// The open PR first, then the one before it.
    prs: Vec<Opened>,
    /// How many chunks have used trees.
    chunks: u64,
}

impl Worker {
    fn new(shelf: ShelfWriter, cap: usize) -> Self {
        Self {
            store: Store {
                shelf,
                kept: 0,
                cap,
            },
            prs: Vec::new(),
            chunks: 0,
        }
    }

    /// Answers chunks, and parses ahead while none waits, until the last `Highlighter` has gone.
    fn run(mut self, jobs: &Receiver<Job>) {
        let mut waiting = VecDeque::new();
        loop {
            waiting.extend(jobs.try_iter());
            if let Some(job) = waiting.pop_front() {
                self.handle(job);
            } else if !self.parse_ahead(jobs, &mut waiting) {
                match jobs.recv() {
                    Ok(job) => waiting.push_back(job),
                    Err(_) => return,
                }
            }
        }
    }

    fn handle(&mut self, job: Job) {
        self.chunks += 1;
        let chunk = self.chunks;
        match job {
            Job::Parse {
                from,
                versions,
                reply,
            } => {
                let mut opened = self.take(from);
                for (version, read) in opened.read(&opened.missing(&versions)) {
                    let tree = match read {
                        Read::Text(language, source) => Parsed::parse(language, source),
                        Read::Plain => None,
                        // Left for the next chunk to ask for again.
                        Read::Failed => continue,
                    };
                    opened.put(&mut self.store, version, Kept::new(tree, chunk));
                }
                let trees = opened.used(&versions, chunk);
                #[expect(
                    clippy::let_underscore_must_use,
                    reason = "this fails only if the chunk has gone, and the trees stay on the shelf"
                )]
                let _ = reply.send(trees);
                self.open(opened);
                self.make_room(chunk);
            }
            Job::Used { from, versions } => {
                let mut opened = self.take(from);
                drop(opened.used(&versions, chunk));
                self.open(opened);
            }
        }
    }

    /// `from`'s diff, taken out of those kept and read from where `from` says, or new.
    fn take(&mut self, from: Source) -> Opened {
        match self
            .prs
            .iter()
            .position(|opened| opened.diff().is(&from.diff))
        {
            Some(at) => {
                let mut opened = self.prs.remove(at);
                opened.source = from;
                opened
            }
            None => Opened::new(from),
        }
    }

    /// Makes `opened` the open PR, ahead of the one open before. A PR opened before that one is dropped,
    /// trees and all.
    fn open(&mut self, opened: Opened) {
        self.prs.insert(0, opened);
        for dropped in self.prs.split_off(PRS.min(self.prs.len())) {
            self.store.kept -= dropped.cost();
        }
        let diffs: Vec<&OpenDiff> = self.prs.iter().map(Opened::diff).collect();
        self.store.shelf.keep(&diffs);
    }

    /// Drops the trees used longest ago until those kept fit in the cap: of those no chunk has used, the
    /// previous PR's first. The trees chunk `chunk` uses stay.
    fn make_room(&mut self, chunk: u64) {
        while self.store.kept > self.store.cap {
            let oldest = self
                .prs
                .iter()
                .enumerate()
                .flat_map(|(at, opened)| {
                    opened
                        .trees
                        .iter()
                        .filter(|(_, kept)| kept.cost > 0 && kept.used < chunk)
                        .map(move |(&version, kept)| (kept.used, Reverse(at), version))
                })
                .min();
            let Some((_, Reverse(at), version)) = oldest else {
                return;
            };
            for opened in self.prs.iter_mut().skip(at).take(1) {
                if let Some(dropped) = opened.trees.remove(&version) {
                    self.store.kept -= dropped.cost;
                }
                self.store.shelf.take(&opened.source.diff, version);
            }
        }
    }

    /// Parses the open PR's next few versions. Once the trees kept reach the cap, it parses no more of
    /// that PR's ahead, so that the chunks' trees don't take turns with the ones parsed ahead. It says
    /// whether there was anything to parse.
    fn parse_ahead(&mut self, jobs: &Receiver<Job>, waiting: &mut VecDeque<Job>) -> bool {
        if self.prs.is_empty() {
            return false;
        }
        let mut opened = self.prs.remove(0);
        if self.store.kept >= self.store.cap {
            opened.ahead.clear();
        }
        let parsed = opened.parse_ahead(&mut self.store, jobs, waiting);
        self.prs.insert(0, opened);
        parsed
    }
}

/// A version as git gave it.
enum Read {
    /// Its language, and its text.
    Text(Language, String),
    /// Tsugiki doesn't highlight its language, or git has no such file.
    Plain,
    /// git failed, so it's worth asking again.
    Failed,
}

impl OpenDiff {
    /// Whether this is the same diff as `other`: of the same PR at the same head, from the same merge base.
    /// A PR retargeted to another branch has another merge base, and its diff another list of files.
    fn is(&self, other: &Self) -> bool {
        self.pr.is(&other.pr) && self.base == other.base
    }

    /// The version and line that row `row` shows, if it shows a line of code.
    fn line(&self, row: u32, shown: &DiffRow) -> Option<(Version, u32)> {
        let (side, line) = match *shown {
            DiffRow::Context { new, .. } | DiffRow::Added { new, .. } => (Side::Head, new),
            DiffRow::Removed { old, .. } => (Side::Base, old),
            DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => return None,
        };
        let file = self
            .diff
            .files
            .partition_point(|file| file.row <= row)
            .checked_sub(1)?;
        Some(((file, side), line))
    }

    /// The versions to parse ahead, in order: those the rows show and Tsugiki highlights, NEWS entries
    /// first, then the rest in the diff's order, each file's merge base before its head. A Generated file
    /// waits for a chunk to ask for it, as it starts collapsed (#181).
    fn ahead(&self) -> VecDeque<Version> {
        let rows = self.diff.rows(0..self.diff.row_count());
        let shown: BTreeSet<Version> = (0..)
            .zip(rows)
            .filter_map(|(row, shown)| self.line(row, shown))
            .map(|(version, _)| version)
            .collect();
        let is_news = |&(file, _): &Version| {
            self.diff
                .files
                .get(file)
                .is_some_and(|file| NewsEntry::is_entry(&file.path))
        };
        let (news, rest): (Vec<Version>, Vec<Version>) = shown
            .into_iter()
            .filter(|&(file, side)| {
                let generated = self.diff.files.get(file).is_some_and(|file| file.generated);
                !generated && self.object((file, side)).is_some()
            })
            .partition(is_news);
        news.into_iter().chain(rest).collect()
    }

    /// The language of `version` and the name git knows it by, if Tsugiki highlights it.
    fn object(&self, (file, side): Version) -> Option<(Language, String)> {
        let file = self.diff.files.get(file)?;
        let (commit, path) = match side {
            Side::Base => (&self.base, file.old_path.as_ref().unwrap_or(&file.path)),
            Side::Head => (&self.pr.head, &file.path),
        };
        let language = Language::of(path)?;
        object_name(commit, path).map(|name| (language, name))
    }

    /// `versions` read from `mirror` with one git.
    fn read(&self, mirror: &Path, versions: &[Version]) -> Vec<(Version, Read)> {
        let objects: Vec<Option<(Language, String)>> = versions
            .iter()
            .map(|&version| self.object(version))
            .collect();
        let names: Vec<&str> = objects
            .iter()
            .flatten()
            .map(|(_, name)| name.as_str())
            .collect();
        let mut blobs = blobs(mirror, &names).into_iter();
        versions
            .iter()
            .zip(objects)
            .map(|(&version, object)| {
                let read = match object.map(|(language, _)| (language, blobs.next())) {
                    None | Some((_, Some(None))) => Read::Plain,
                    Some((language, Some(Some(text)))) => Read::Text(language, text),
                    Some((_, None)) => Read::Failed,
                };
                (version, read)
            })
            .collect()
    }
}

/// The name git knows `path` at `commit` by, unless the path has a line break: git reads a name a line.
fn object_name(commit: &CommitSha, path: &str) -> Option<String> {
    (!path.contains('\n')).then(|| format!("{}:{path}", commit.as_str()))
}

/// The files `names` names, read from `mirror` with one `git cat-file --batch`: the text of each, or
/// nothing for one git has no blob for. If git fails, the list stops short.
pub(crate) fn blobs(mirror: &Path, names: &[&str]) -> Vec<Option<String>> {
    if names.is_empty() {
        return Vec::new();
    }
    let input: String = names.iter().map(|name| format!("{name}\n")).collect();
    let args = [
        OsStr::new("-C"),
        mirror.as_os_str(),
        OsStr::new("cat-file"),
        OsStr::new("--batch"),
    ];
    match live::read_git_bytes(&args, input.into_bytes()) {
        Ok(output) => parse_batch(&output),
        Err(cause) => {
            let mirror = mirror.display();
            log::warn!("Tsugiki can't read files from {mirror}: {cause}");
            Vec::new()
        }
    }
}

/// What `git cat-file --batch` printed: for each name, `<id> <type> <size>`, a line feed, the object and a
/// line feed, or a line such as `<name> missing`. Only a blob gives text. Output cut short stops the list.
fn parse_batch(mut output: &[u8]) -> Vec<Option<String>> {
    let mut blobs = Vec::new();
    while let Some(end) = output.iter().position(|&byte| byte == b'\n') {
        let header = String::from_utf8_lossy(output.get(..end).unwrap_or_default());
        output = output.get(end + 1..).unwrap_or_default();
        let fields: Vec<&str> = header.split(' ').collect();
        let found = match fields.as_slice() {
            [_, kind, size] => size.parse::<usize>().ok().map(|size| (*kind, size)),
            _ => None,
        };
        let Some((kind, size)) = found else {
            log::warn!("Tsugiki can't read a file to highlight it: git says {header}");
            blobs.push(None);
            continue;
        };
        let Some(object) = output.get(..size) else {
            break;
        };
        blobs.push((kind == "blob").then(|| String::from_utf8_lossy(object).into_owned()));
        output = output.get(size + 1..).unwrap_or_default();
    }
    blobs
}

#[cfg(test)]
mod tests;
