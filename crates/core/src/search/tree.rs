//! The PR's head as one tree: the Lookup checkout's files on disk, the base's and those the PR adds, with
//! the head's text of each file the PR changes read from the mirror's objects, and none of the files it
//! deletes. git in the checkout reads the mirror's objects, as a worktree shares them.
//!
//! Nothing holds the checkout still while a search reads it. The worker moves it only when another PR
//! opens, and the window then asks a new query, which cancels this one.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use grep_searcher::Searcher;

use super::engine::{self, Matcher};
use super::{FileHits, LineHit, MOST_MATCHES};
use crate::github::live;
use crate::mirror::LookupCheckout;

/// The threads that read files from disk. One more reads the PR's files from the mirror, so a search runs
/// on 4 at most, and the thread that started it waits.
const DISK_THREADS: usize = 3;

/// How a search over the tree ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    /// It read every file.
    Done,
    /// It found [`MOST_MATCHES`] matches, and stopped there.
    Stopped,
}

/// Searches each file of the head in `checkout` with `matcher`, and hands `found` the files that hold a
/// match, in batches as they finish. Once `cancelled` is set it reads no more files, and says it is done,
/// as nobody reads what it says then. A git that fails, listing or reading the PR's files, fails it. A file
/// it can't read from disk is logged and left out.
pub(super) fn search(
    checkout: &LookupCheckout,
    matcher: &Matcher,
    cancelled: &AtomicBool,
    found: &mut dyn FnMut(Vec<FileHits>),
) -> io::Result<Outcome> {
    let folder = checkout.folder.as_path();
    let files = files(folder, checkout)?;
    let (disk, changed): (Vec<_>, Vec<_>) = files.into_iter().partition(|file| file.blob.is_none());
    let run = Run {
        matcher,
        cancelled,
        matches: AtomicU32::new(0),
        stopped: AtomicBool::new(false),
    };
    let next = AtomicUsize::new(0);
    let searched = thread::scope(|scope| {
        let (send, hits) = mpsc::channel();
        for _ in 0..DISK_THREADS {
            let send = send.clone();
            let (run, disk, next) = (&run, &disk, &next);
            // The scope joins each thread as it ends.
            let _reader = scope.spawn(move || {
                let mut hand = Hand::new(send);
                let next_file = || disk.get(next.fetch_add(1, Ordering::Relaxed));
                while let Some(file) = next_file().filter(|_| !run.over()) {
                    match std::fs::read(folder.join(&file.path)) {
                        Ok(bytes) => run.search(&mut hand, &file.path, &bytes),
                        Err(cause) => {
                            let path = &file.path;
                            log::warn!("Tsugiki's text search can't read {path}: {cause}");
                        }
                    }
                }
            });
        }
        // The disk's threads search while git reads the PR's files.
        let ids: Vec<&BlobId> = (changed.iter())
            .filter_map(|file| file.blob.as_ref())
            .collect();
        let blobs = blobs(folder, &ids)?;
        let (run, changed) = (&run, &changed);
        let _blob_reader = scope.spawn(move || {
            let mut hand = Hand::new(send);
            // A blob git has no text for holds nothing to search.
            let read = (changed.iter().zip(blobs))
                .take_while(|_| !run.over())
                .filter_map(|(file, bytes)| bytes.map(|bytes| (file, bytes)));
            for (file, bytes) in read {
                run.search(&mut hand, &file.path, &bytes);
            }
        });
        // Each batch that has come asks git which of its files are generated, and goes on, so the first
        // results wait for one short git and not for one over the whole tree.
        while let Ok(first) = hits.recv() {
            let mut batch: Vec<FileHits> = [first].into_iter().chain(hits.try_iter()).collect();
            let paths: Vec<&str> = batch.iter().map(|hits| hits.path.as_str()).collect();
            let marked: HashSet<String> = generated(folder, &paths)?
                .into_iter()
                .map(str::to_owned)
                .collect();
            for hits in &mut batch {
                hits.generated = marked.contains(&hits.path);
            }
            found(batch);
        }
        Ok::<(), io::Error>(())
    });
    searched?;
    Ok(if run.stopped.load(Ordering::Relaxed) {
        Outcome::Stopped
    } else {
        Outcome::Done
    })
}

/// What the threads of one search share.
struct Run<'a> {
    matcher: &'a Matcher,
    cancelled: &'a AtomicBool,
    /// The matches the files handed on hold.
    matches: AtomicU32,
    /// Whether the matches reached [`MOST_MATCHES`].
    stopped: AtomicBool,
}

impl Run<'_> {
    fn over(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || self.stopped.load(Ordering::Relaxed)
    }

    /// Searches the file at `path`, and sends its matches on, as many as fit under [`MOST_MATCHES`].
    fn search(&self, hand: &mut Hand, path: &str, bytes: &[u8]) {
        let Some(mut lines) = self.matcher.lines(&mut hand.searcher, bytes) else {
            return;
        };
        let count: u32 = lines.iter().map(LineHit::matches).sum();
        if count == 0 {
            return;
        }
        let before = self.matches.fetch_add(count, Ordering::Relaxed);
        let room = MOST_MATCHES.saturating_sub(before);
        if room < count {
            self.stopped.store(true, Ordering::Relaxed);
            keep_first(&mut lines, room);
        }
        if lines.is_empty() {
            return;
        }
        let hits = FileHits {
            path: path.to_owned(),
            generated: false,
            lines,
        };
        // The thread that hears them stops listening only once every thread has ended.
        let _heard = hand.send.send(hits);
    }
}

/// What one thread searches with: its own searcher, and where it sends what it finds.
struct Hand {
    searcher: Searcher,
    send: mpsc::Sender<FileHits>,
}

impl Hand {
    fn new(send: mpsc::Sender<FileHits>) -> Self {
        Self {
            searcher: engine::searcher(),
            send,
        }
    }
}

/// Keeps the first `room` matches of `lines`, and the lines that hold them.
fn keep_first(lines: &mut Vec<LineHit>, mut room: u32) {
    lines.retain_mut(|line| {
        let kept = line.matches().min(room);
        line.ranges
            .truncate(usize::try_from(kept).unwrap_or(usize::MAX));
        room = room.saturating_sub(kept);
        kept > 0
    });
}

/// The name git knows a file's text by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BlobId(String);

/// A file of the head: its path from the top, and the blob it has to be read from if the PR changes it, as
/// the checkout holds the base's version. The checkout holds the head's version of a file the PR adds.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct TreeFile {
    pub(super) path: String,
    pub(super) blob: Option<BlobId>,
}

/// The regular files of the head in `checkout`: each with its blob where the base holds another version.
/// Links and submodules hold no text of their own.
pub(super) fn files(folder: &Path, checkout: &LookupCheckout) -> io::Result<Vec<TreeFile>> {
    let base: HashMap<String, BlobId> = tree(folder, checkout.at.base.as_str())?
        .into_iter()
        .collect();
    let head = tree(folder, checkout.at.head.as_str())?;
    Ok(head
        .into_iter()
        .map(|(path, blob)| {
            let changed = base.get(&path).is_some_and(|at_base| *at_base != blob);
            TreeFile {
                path,
                blob: changed.then_some(blob),
            }
        })
        .collect())
}

/// The regular files at `commit`, each path with its blob.
fn tree(folder: &Path, commit: &str) -> io::Result<Vec<(String, BlobId)>> {
    let args = ["-C", "", "ls-tree", "-r", "-z", "--full-tree", commit].map(OsStr::new);
    let mut args = args;
    args[1] = folder.as_os_str();
    Ok(parse_tree(&live::read_git_bytes(&args, Vec::new())?))
}

/// The regular files in what `git ls-tree -r -z` printed: for each entry the mode, the type and the blob,
/// then a tab, the path and a NUL. A link or a submodule isn't one.
fn parse_tree(output: &[u8]) -> Vec<(String, BlobId)> {
    output
        .split(|&byte| byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (about, path) = entry.split_once('\t')?;
            match about.split(' ').collect::<Vec<_>>().as_slice() {
                ["100644" | "100755", "blob", blob] => {
                    Some((path.to_owned(), BlobId((*blob).to_owned())))
                }
                _ => None,
            }
        })
        .collect()
}

/// The blobs `ids` names, read with one `git cat-file --batch`: the bytes of each, or nothing for one git
/// has no blob for. Output cut short ends the list.
fn blobs(folder: &Path, ids: &[&BlobId]) -> io::Result<Vec<Option<Vec<u8>>>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let input: Vec<u8> = (ids.iter())
        .flat_map(|BlobId(id)| id.bytes().chain(*b"\n"))
        .collect();
    let args = [
        OsStr::new("-C"),
        folder.as_os_str(),
        OsStr::new("cat-file"),
        OsStr::new("--batch"),
    ];
    let output = live::read_git_bytes(&args, input)?;
    Ok(parse_batch(&output))
}

/// What `git cat-file --batch` printed: for each name, `<id> <type> <size>`, a line feed, the object and a
/// line feed, or a line such as `<id> missing`.
fn parse_batch(mut output: &[u8]) -> Vec<Option<Vec<u8>>> {
    let mut objects = Vec::new();
    while let Some(end) = output.iter().position(|&byte| byte == b'\n') {
        let header = String::from_utf8_lossy(output.get(..end).unwrap_or_default());
        output = output.get(end.saturating_add(1)..).unwrap_or_default();
        let size = match header.split(' ').collect::<Vec<_>>().as_slice() {
            [_, "blob", size] => size.parse::<usize>().ok(),
            _ => None,
        };
        let Some(size) = size else {
            objects.push(None);
            continue;
        };
        let Some(object) = output.get(..size) else {
            break;
        };
        objects.push(Some(object.to_vec()));
        output = output.get(size.saturating_add(1)..).unwrap_or_default();
    }
    objects
}

/// Which of `paths` the attributes in the checkout mark `linguist-generated`, set or `true`, as the Files
/// tab marks a Generated file. It reads the `.gitattributes` in the checkout, the base's and any the PR adds,
/// and not the user's or the computer's. The Files tab reads the Target branch's, which a PR's new file
/// doesn't change: making an index of the base to read them would cost more than the search.
fn generated<'p>(folder: &Path, paths: &[&'p str]) -> io::Result<HashSet<&'p str>> {
    let input: Vec<u8> = paths
        .iter()
        .flat_map(|path| path.bytes().chain([0]))
        .collect();
    let args = [
        OsStr::new("-C"),
        folder.as_os_str(),
        OsStr::new("-c"),
        OsStr::new("core.attributesFile="),
        OsStr::new("check-attr"),
        OsStr::new("--stdin"),
        OsStr::new("-z"),
        OsStr::new("linguist-generated"),
    ];
    let envs = [("GIT_ATTR_NOSYSTEM", OsStr::new("1"))];
    let output = live::read_git_bytes_with_env(&args, input, &envs)?;
    let output = String::from_utf8_lossy(&output);
    // Each answer is the path, the attribute and its value, each ended by a NUL.
    let fields: Vec<&str> = output.split('\0').collect();
    let marked: HashSet<&str> = fields
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|[_, _, value]| matches!(*value, "set" | "true"))
        .map(|[path, _, _]| *path)
        .collect();
    Ok(paths
        .iter()
        .copied()
        .filter(|path| marked.contains(path))
        .collect())
}

#[cfg(test)]
mod tests;
