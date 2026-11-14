//! The code of a hit's file, which the Definition peek shows a few rows of at a time (#439). A file of the
//! Repository comes from its Mirror at the hit's commit, and one outside it from disk, if a server named it.

use std::collections::HashSet;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use specta::Type;

use crate::diff;
use crate::error::{Error, ErrorKind};
use crate::github::CommitSha;

#[cfg(test)]
mod tests;

/// The most lines one read hands the window.
const MOST_LINES: u32 = 2000;

/// A file a hit is in, as the window names it to read its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HitFile {
    /// A file of the Repository at a commit, read from its Mirror.
    Repository { path: String, commit: CommitSha },
    /// A file outside the Repository, read from disk.
    Outside { path: PathBuf },
}

/// Some lines of a hit's file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Code {
    /// How many lines the whole file has.
    pub count: u32,
    /// The lines asked for that the file holds, without their line endings.
    pub lines: Vec<String>,
}

/// The paths outside the Repository that a server has named in a hit: the only files outside it that the
/// window can read.
#[derive(Debug, Default)]
pub(super) struct Named(Mutex<HashSet<PathBuf>>);

impl Named {
    pub(super) fn add(&self, path: PathBuf) {
        let _new = self.lock().insert(path);
    }

    fn has(&self, path: &Path) -> bool {
        self.lock().contains(path)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashSet<PathBuf>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Lines `lines` of `file`, counted from 1: one of the Repository whose Mirror is at `mirror`, or one
/// outside it that a server has named.
pub(super) fn read(
    named: &Named,
    mirror: &Path,
    file: &HitFile,
    lines: Range<u32>,
) -> Result<Code, Error> {
    let text = match file {
        HitFile::Repository { path, commit } => {
            let short = commit.as_str().get(..7).unwrap_or_default();
            let missing = format!("Tsugiki's copy of the Repository has no {path} at {short}.");
            let name = format!("{}:{path}", commit.as_str());
            let blob = (!path.contains('\n')).then(|| diff::blobs(mirror, &[&name]));
            let text = blob.and_then(|blobs| blobs.into_iter().next().flatten());
            text.ok_or_else(|| Error::new(ErrorKind::NotFound, &missing))?
        }
        HitFile::Outside { path } => {
            let shown = path.display();
            if !named.has(path) {
                let unnamed = format!("No language server named {shown}.");
                return Err(Error::new(ErrorKind::NotFound, &unnamed));
            }
            let bytes = fs::read(path).map_err(|cause| {
                let message = format!("Tsugiki can't read {shown}: {cause}");
                Error::new(ErrorKind::NotFound, &message)
            })?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };
    Ok(some_lines(&text, lines))
}

/// Lines `lines` of `text`, counted from 1, with how many it has.
fn some_lines(text: &str, lines: Range<u32>) -> Code {
    let first = lines.start.max(1);
    let end = lines.end.min(first.saturating_add(MOST_LINES));
    let skip = usize::try_from(first - 1).unwrap_or(usize::MAX);
    let take = usize::try_from(end.saturating_sub(first)).unwrap_or(0);
    Code {
        count: u32::try_from(text.lines().count()).unwrap_or(u32::MAX),
        lines: (text.lines().skip(skip).take(take))
            .map(str::to_owned)
            .collect(),
    }
}
