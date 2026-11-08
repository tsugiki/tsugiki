//! Tsugiki's own search in a PR's diff (#186). It reads every row, so it finds text in the rows the window
//! hasn't asked for yet, and in the Generated files it shows closed.

use serde::Serialize;
use specta::Type;

use super::{Diff, DiffRow};

/// The most matches a search returns. A query of one common letter finds many more in a large diff, and
/// the window would hold each.
pub const MOST_MATCHES: usize = 10_000;

/// What a search found, in the order the rows come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Found {
    pub matches: Vec<Match>,
    /// Whether the diff holds more than [`MOST_MATCHES`] matches, and the search stopped there.
    pub more: bool,
}

/// Where a search found its text: the diff's row, and from `start` to `end` in the row's code in UTF-16
/// code units, as the webview and a [`crate::highlight::Span`] count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub struct Match {
    pub row: u32,
    pub start: u32,
    pub end: u32,
}

impl Diff {
    /// Where the code of the rows holds `query`, whatever the case, as VS Code finds by default. Matches
    /// don't overlap. Headers, `@@` lines and git's notes hold no code, so it doesn't look there.
    pub fn find(&self, query: &str) -> Found {
        let query: Vec<char> = query.chars().map(folded).collect();
        let mut matches = Vec::new();
        if query.is_empty() {
            return Found {
                matches,
                more: false,
            };
        }
        for (row, text) in (0_u32..)
            .zip(&self.rows)
            .filter_map(|(row, line)| Some((row, code(line)?)))
        {
            for (start, end) in found_in(text, &query) {
                if matches.len() == MOST_MATCHES {
                    return Found {
                        matches,
                        more: true,
                    };
                }
                matches.push(Match { row, start, end });
            }
        }
        Found {
            matches,
            more: false,
        }
    }
}

/// The code of `row`, if it holds code.
fn code(row: &DiffRow) -> Option<&str> {
    match row {
        DiffRow::Context { text, .. }
        | DiffRow::Added { text, .. }
        | DiffRow::Removed { text, .. } => Some(text),
        DiffRow::File { .. } | DiffRow::Hunk { .. } | DiffRow::Note { .. } => None,
    }
}

/// `character` in lower case, when that is one character; otherwise `character` itself, so a match keeps
/// the length it has in the row.
fn folded(character: char) -> char {
    let mut lower = character.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(one), None) => one,
        _ => character,
    }
}

/// Where `text` holds `query`, already [`folded`], from left to right without overlaps, in UTF-16 code
/// units.
fn found_in(text: &str, query: &[char]) -> Vec<(u32, u32)> {
    // Each character, folded, and where it starts.
    let mut characters = Vec::new();
    let mut at = 0_u32;
    for character in text.chars() {
        characters.push((folded(character), at));
        at = at.saturating_add(u32::try_from(character.len_utf16()).unwrap_or(2));
    }
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(window) = characters.get(from..from.saturating_add(query.len())) {
        if window.iter().map(|(character, _)| character).eq(query) {
            let start = window.first().map_or(at, |(_, start)| *start);
            let next = from.saturating_add(query.len());
            let end = characters.get(next).map_or(at, |(_, start)| *start);
            found.push((start, end));
            from = next;
        } else {
            from = from.saturating_add(1);
        }
    }
    found
}

#[cfg(test)]
mod tests;
