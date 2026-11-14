//! The code of a hit's file, which the Definition peek shows a few rows of at a time (#439), and its spans
//! (#455). A file of the Repository comes from its Mirror at the hit's commit, and one outside it from disk,
//! if a server named it.

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::diff::{self, Highlighter, HitFile, HitLines};
use crate::error::{Error, ErrorKind};
use crate::highlight::Span;

#[cfg(test)]
mod tests;

/// The most lines one read hands the window.
const MOST_LINES: u32 = 2000;

/// A read of a file's lines, which every caller that asks for the same file while it runs waits for.
type Read = Arc<OnceLock<Result<Arc<[String]>, Error>>>;

/// Some lines of a hit's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Code {
    /// How many lines the whole file has.
    pub count: u32,
    /// The lines asked for that the file holds, without their line endings.
    pub lines: Vec<String>,
}

/// The files the peek reads: the paths outside the Repository that a server has named in a hit, the only
/// files outside it that the window can read, and the last file read, as the peek reads one file a chunk
/// at a time. The window asks for a chunk's lines and its spans together, and the two share one read.
#[derive(Debug, Default)]
pub(super) struct HitFiles {
    named: Mutex<HashSet<PathBuf>>,
    last: Mutex<Option<(HitFile, Read)>>,
    /// How many times a file was read, for the tests.
    #[cfg(test)]
    reads: AtomicUsize,
}

impl HitFiles {
    pub(super) fn name(&self, path: PathBuf) {
        let _new = lock(&self.named).insert(path);
    }

    /// The lines `wanted` names: of a file of the Repository whose Mirror is at `mirror`, or of one outside
    /// it that a server has named.
    pub(super) fn read(&self, mirror: &Path, wanted: &HitLines) -> Result<Code, Error> {
        let HitLines { file, lines } = wanted;
        self.check_named(file)?;
        Ok(some_lines(&self.lines_of(mirror, file)?, lines.clone()))
    }

    /// The spans of the lines `wanted` names, which `highlighter` reads from the file's whole tree, as the
    /// diff's rows read theirs. It reads the file as `read` does, and only when `highlighter` needs it.
    pub(super) fn spans(
        &self,
        highlighter: &Highlighter,
        mirror: &Path,
        wanted: &HitLines,
    ) -> Result<Vec<Vec<Span>>, Error> {
        let HitLines { file, lines } = wanted;
        self.check_named(file)?;
        // Joined with line feeds, a lone `\r` ends a line for tree-sitter too, so the spans of each line
        // land on the line `read` gives.
        let text = || Ok(self.lines_of(mirror, file)?.join("\n"));
        let wanted = HitLines {
            file: file.clone(),
            lines: at_most(lines.clone()),
        };
        highlighter.whole_spans(mirror, &wanted, text)
    }

    /// Fails for a file outside the Repository that no server has named.
    fn check_named(&self, file: &HitFile) -> Result<(), Error> {
        if let HitFile::Outside { path } = file
            && !lock(&self.named).contains(path)
        {
            let unnamed = format!("No language server named {}.", path.display());
            return Err(Error::new(ErrorKind::NotFound, &unnamed));
        }
        Ok(())
    }

    /// `file`'s lines, from the last file read if it's the same one, or from the read of it under way. A
    /// read that fails is forgotten, so the next chunk reads again.
    fn lines_of(&self, mirror: &Path, file: &HitFile) -> Result<Arc<[String]>, Error> {
        let read = {
            let mut last = lock(&self.last);
            match &*last {
                Some((kept, read)) if kept == file => Arc::clone(read),
                _ => {
                    let read = Read::default();
                    *last = Some((file.clone(), Arc::clone(&read)));
                    read
                }
            }
        };
        let lines = read
            .get_or_init(|| {
                #[cfg(test)]
                let _before = self.reads.fetch_add(1, Ordering::Relaxed);
                text_of(mirror, file).map(|text| split(&text).into())
            })
            .clone();
        if lines.is_err() {
            let mut last = lock(&self.last);
            if last
                .as_ref()
                .is_some_and(|(_, kept)| Arc::ptr_eq(kept, &read))
            {
                *last = None;
            }
        }
        lines
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The text of `file`, from the Mirror at `mirror` or from disk.
fn text_of(mirror: &Path, file: &HitFile) -> Result<String, Error> {
    match file {
        HitFile::Repository { path, commit } => {
            if !mirror.is_dir() {
                let none = "Tsugiki has no copy of this Repository yet.";
                return Err(Error::new(ErrorKind::NotFound, none));
            }
            let short = commit.as_str().get(..7).unwrap_or_default();
            let missing = format!("Tsugiki's copy of the Repository has no {path} at {short}.");
            let name = diff::object_name(commit, path);
            let blob = name.and_then(|name| diff::blobs(mirror, &[&name]).into_iter().next());
            blob.flatten()
                .ok_or_else(|| Error::new(ErrorKind::NotFound, &missing))
        }
        HitFile::Outside { path } => {
            let bytes = fs::read(path).map_err(|cause| {
                let message = format!("Tsugiki can't read {}: {cause}", path.display());
                Error::new(ErrorKind::NotFound, &message)
            })?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

/// `text`'s lines, which end at a `\r\n`, a `\n` or a lone `\r`, as LSP counts them, so a hit's line
/// number lands on its line.
fn split(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut rest = text;
    while let Some(end) = rest.find(['\r', '\n']) {
        let (line, after) = rest.split_at(end);
        lines.push(line.to_owned());
        rest = after
            .strip_prefix("\r\n")
            .or_else(|| after.get(1..))
            .unwrap_or_default();
    }
    if !rest.is_empty() {
        lines.push(rest.to_owned());
    }
    lines
}

/// Lines `lines` of `text`, counted from 1, with how many it has.
fn some_lines(text: &[String], lines: Range<u32>) -> Code {
    let Range { start: first, end } = at_most(lines);
    let skip = usize::try_from(first - 1).unwrap_or(usize::MAX);
    let take = usize::try_from(end.saturating_sub(first)).unwrap_or(0);
    Code {
        count: u32::try_from(text.len()).unwrap_or(u32::MAX),
        lines: text.iter().skip(skip).take(take).cloned().collect(),
    }
}

/// The lines of `lines` one read hands the window: from line 1 at the earliest, and `MOST_LINES` at most.
fn at_most(lines: Range<u32>) -> Range<u32> {
    let first = lines.start.max(1);
    first..lines.end.min(first.saturating_add(MOST_LINES))
}
