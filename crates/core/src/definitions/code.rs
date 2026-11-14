//! The code of a hit's file, which the Definition peek shows a few rows of at a time (#439), and its spans
//! (#455). A file of the Repository comes from its Mirror at the hit's commit, and one outside it from disk,
//! if a server named it.

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::diff::{self, Highlighter, HitFile};
use crate::error::{Error, ErrorKind};
use crate::highlight::Span;

#[cfg(test)]
mod tests;

/// The most lines one read hands the window.
const MOST_LINES: u32 = 2000;

/// Lines of a hit's file, counted from 1, as the window asks for their spans (#455).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitLines {
    pub file: HitFile,
    pub lines: Range<u32>,
}

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
/// at a time.
#[derive(Debug, Default)]
pub(super) struct HitFiles {
    named: Mutex<HashSet<PathBuf>>,
    last: Mutex<Option<(HitFile, Arc<[String]>)>>,
}

impl HitFiles {
    pub(super) fn name(&self, path: PathBuf) {
        let _new = lock(&self.named).insert(path);
    }

    /// Lines `lines` of `file`, counted from 1: one of the Repository whose Mirror is at `mirror`, or one
    /// outside it that a server has named.
    pub(super) fn read(
        &self,
        mirror: &Path,
        file: &HitFile,
        lines: Range<u32>,
    ) -> Result<Code, Error> {
        Ok(some_lines(&self.lines_of(mirror, file)?, lines))
    }

    /// The spans of lines `lines` of `file`, counted from 1, which `highlighter` reads from the file parsed
    /// whole, as the diff's rows read theirs. It reads the file as `read` does, and only when `highlighter`
    /// doesn't keep its tree.
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
        highlighter.whole_spans(file, at_most(lines.clone()), text)
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

    /// `file`'s lines, from the last file read if it's the same one.
    fn lines_of(&self, mirror: &Path, file: &HitFile) -> Result<Arc<[String]>, Error> {
        self.check_named(file)?;
        let last = lock(&self.last).clone();
        Ok(match last {
            Some((read, text)) if read == *file => text,
            _ => {
                let text: Arc<[String]> = split(&text_of(mirror, file)?).into();
                *lock(&self.last) = Some((file.clone(), Arc::clone(&text)));
                text
            }
        })
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
