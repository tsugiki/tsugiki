//! The matching itself: a query turned into ripgrep's matcher, and the lines of one file that hold a match,
//! ready for the window.

use grep_matcher::{LineTerminator, Matcher as _};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Bytes;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};

use super::{BadQuery, Case, Cut, LineHit, MatchRange, Query, Syntax, Words};

/// The most characters a result shows of its line. A longer line is cut around its first match.
pub const LONGEST_LINE: usize = 250;

/// The characters a cut line keeps before its first match, unless the line ends sooner.
const LEAD: usize = 50;

/// A query ready to run on any number of files, from any number of threads.
#[derive(Debug, Clone)]
pub(super) struct Matcher {
    regex: RegexMatcher,
}

impl Matcher {
    /// `query` as ripgrep's matcher. Lines end at `\n`, and `$` matches before a `\r\n` too: `crlf` makes
    /// that the matcher's line terminator, which [`searcher`] has to share.
    pub(super) fn new(query: &Query) -> Result<Self, BadQuery> {
        if query.text.is_empty() {
            return Err(BadQuery {
                message: "There's nothing to search for.".to_owned(),
            });
        }
        let regex = RegexMatcherBuilder::new()
            .fixed_strings(query.syntax == Syntax::Literal)
            .case_insensitive(query.case == Case::Ignored)
            .word(query.words == Words::Whole)
            .crlf(true)
            .build(&query.text)
            .map_err(|cause| BadQuery {
                message: cause.to_string(),
            })?;
        Ok(Self { regex })
    }

    /// The lines of `bytes` that hold a match, in order, or None for a binary file, which holds a NUL
    /// anywhere, as git and ripgrep tell one. A match that holds no character doesn't count.
    /// `searcher` is the thread's own, from [`searcher`].
    pub(super) fn lines(&self, searcher: &mut Searcher, bytes: &[u8]) -> Option<Vec<LineHit>> {
        if bytes.contains(&0) {
            return None;
        }
        let mut hits = Vec::new();
        let sink = Bytes(|number, line| {
            if let Some(hit) = self.hit(number, line) {
                hits.push(hit);
            }
            Ok(true)
        });
        // Searching a slice reads no file, and the sink fails on nothing, so only the matcher could fail,
        // and Rust's regex never does.
        let _never_fails = searcher.search_slice(&self.regex, bytes, sink);
        Some(hits)
    }

    /// Line `number`, which the searcher found a match on, with where each match is in UTF-16 code units.
    fn hit(&self, number: u64, line: &[u8]) -> Option<LineHit> {
        let line = line.strip_suffix(b"\n").unwrap_or(line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        // Text that isn't UTF-8 is matched again as the window gets it, with replacements, so the ranges
        // fall where it shows them.
        let text = String::from_utf8_lossy(line);
        let mut found = Vec::new();
        // Rust's regex never fails to match.
        let _never_fails = self.regex.find_iter(text.as_bytes(), |found_at| {
            if !found_at.is_empty() {
                found.push((found_at.start(), found_at.end()));
            }
            true
        });
        if found.is_empty() {
            return None;
        }
        Some(cut(number, &text, &found))
    }
}

/// A searcher for one thread's files, which ends lines as each [`Matcher`] does. Building one allocates its
/// buffers, so a thread builds one and keeps it.
pub(super) fn searcher() -> Searcher {
    SearcherBuilder::new()
        .line_terminator(LineTerminator::crlf())
        .line_number(true)
        .binary_detection(BinaryDetection::none())
        .build()
}

/// Line `number` of `text`, cut to [`LONGEST_LINE`] characters around the first match if it is longer, with
/// `found`, its matches in bytes, as ranges of UTF-16 code units in what it keeps.
fn cut(number: u64, text: &str, found: &[(usize, usize)]) -> LineHit {
    // Where each character starts in bytes and in UTF-16 code units, and both ends of the line.
    let mut starts: Vec<(usize, u32)> = Vec::new();
    let mut units = 0_u32;
    for (byte, character) in text.char_indices() {
        starts.push((byte, units));
        units = units.saturating_add(u32::try_from(character.len_utf16()).unwrap_or(2));
    }
    let characters = starts.len();
    starts.push((text.len(), units));
    let unit_at = |byte: usize| -> u32 {
        let at = starts.partition_point(|(start, _)| *start < byte);
        starts.get(at).map_or(units, |(_, unit)| *unit)
    };
    let first = found.first().map_or(0, |(start, _)| *start);
    let first = starts.partition_point(|(start, _)| *start < first);
    let from = if characters <= LONGEST_LINE {
        0
    } else {
        first
            .saturating_sub(LEAD)
            .min(characters.saturating_sub(LONGEST_LINE))
    };
    let to = from.saturating_add(LONGEST_LINE).min(characters);
    let ((from_byte, from_unit), (to_byte, to_unit)) = (
        starts.get(from).copied().unwrap_or_default(),
        starts.get(to).copied().unwrap_or_default(),
    );
    let ranges = found
        .iter()
        .map(|(start, end)| (unit_at(*start).max(from_unit), unit_at(*end).min(to_unit)))
        .filter(|(start, end)| start < end)
        .map(|(start, end)| MatchRange {
            start: start.saturating_sub(from_unit),
            end: end.saturating_sub(from_unit),
        })
        .collect();
    LineHit {
        line: u32::try_from(number).unwrap_or(u32::MAX),
        text: text.get(from_byte..to_byte).unwrap_or_default().to_owned(),
        cut: match (from > 0, to < characters) {
            (false, false) => Cut::Neither,
            (true, false) => Cut::Start,
            (false, true) => Cut::End,
            (true, true) => Cut::Both,
        },
        ranges,
    }
}

#[cfg(test)]
mod tests;
