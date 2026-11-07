//! The spans of a chunk of diff rows (#178). Each line a row shows is in one version of its file: a removed
//! line in the merge base's, and the others in the head's. Each version is read from the mirror and parsed
//! whole the first time a chunk needs it, and kept with the diff on show.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use super::{DiffRow, OpenDiff, git};
use crate::highlight::{Language, Parsed, Span};

/// Which version of a file a line is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Side {
    Base,
    Head,
}

/// One version of one of the diff's files: its place in the diff's list of files, and which side.
type Version = (usize, Side);

/// The most lines between two of a chunk's lines that are still highlighted in one read. Hunks further
/// apart are read apart, so a chunk never pays for the code between two distant hunks.
const GAP: u32 = 32;

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

/// The versions of the diff's files parsed so far, and those that can't be: a language Tsugiki doesn't
/// highlight, or a version git couldn't give. The lock is held only to look or to add, never while parsing.
#[derive(Debug, Default)]
pub(super) struct Trees {
    parsed: Mutex<HashMap<Version, Option<Arc<Parsed>>>>,
}

impl Trees {
    /// `version` parsed, as kept or as `parse` gives it. Two chunks that ask at once may both parse it.
    fn get_or_parse(
        &self,
        version: Version,
        parse: impl FnOnce() -> Option<Parsed>,
    ) -> Option<Arc<Parsed>> {
        if let Some(kept) = self.lock().get(&version) {
            return kept.clone();
        }
        let parsed = parse().map(Arc::new);
        self.lock().entry(version).or_insert(parsed).clone()
    }

    /// A panic while the lock was held left nothing half done: the lock only guards a look or an insert.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Version, Option<Arc<Parsed>>>> {
        self.parsed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl OpenDiff {
    /// The spans of each row in `range`, read from the versions of its files in `mirror`. A row that shows
    /// no line of code, or whose file Tsugiki doesn't highlight, has none.
    pub fn spans(&self, mirror: &Path, range: Range<u32>) -> Vec<Vec<Span>> {
        let first = range.start;
        let lines: Vec<Option<(Version, u32)>> = (first..)
            .zip(self.diff.rows(range))
            .map(|(row, shown)| self.line(row, shown))
            .collect();
        // The lines each version shows in this chunk, in order.
        let mut wanted: BTreeMap<Version, Vec<u32>> = BTreeMap::new();
        for &(version, line) in lines.iter().flatten() {
            wanted.entry(version).or_default().push(line);
        }
        let mut spans: HashMap<(Version, u32), Vec<Span>> = HashMap::new();
        for (version, mut shown) in wanted {
            let Some(parsed) = self
                .trees
                .get_or_parse(version, || self.parse(mirror, version))
            else {
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

    /// `version` read from `mirror` and parsed, if Tsugiki highlights its language and git can give it.
    fn parse(&self, mirror: &Path, (file, side): Version) -> Option<Parsed> {
        let file = self.diff.files.get(file)?;
        let (commit, path) = match side {
            Side::Base => (&self.base, file.old_path.as_ref().unwrap_or(&file.path)),
            Side::Head => (&self.pr.head, &file.path),
        };
        let language = Language::of(path)?;
        let object = format!("{}:{path}", commit.as_str());
        match git(mirror, &["cat-file", "blob", &object]) {
            Ok(source) => Parsed::parse(language, source),
            Err(cause) => {
                let mirror = mirror.display();
                log::warn!("Tsugiki can't read {object} from {mirror} to highlight it: {cause}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_close_together_are_read_in_one_run_and_distant_hunks_apart() {
        let lines = [3, 4, 5, 9, 10, 5000, 5001];

        assert_eq!(runs(&lines), [3..11, 5000..5002]);
        assert_eq!(runs(&[]), Vec::<Range<u32>>::new());
    }
}
