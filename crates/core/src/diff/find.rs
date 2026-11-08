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
mod tests {
    use super::*;

    /// A diff of `a.py`: a header, an `@@` line, then `x = 1` and `Ä x` removed, `X = 2` and `y = 3` added,
    /// and `😀 x` as context.
    fn diff() -> Diff {
        let patch = "diff --git a/a.py b/a.py\n@@ -1,3 +1,4 @@ def x\n-x = 1\n-Ä x\n+X = 2\n+y = 3\n 😀 x\n";
        Diff::parse("3\t2\ta.py\0", patch).unwrap()
    }

    fn hit(row: u32, start: u32, end: u32) -> Match {
        Match { row, start, end }
    }

    #[test]
    fn a_search_finds_the_text_in_any_case_in_the_code_of_each_row() {
        let found = diff().find("x");

        assert_eq!(
            found.matches,
            [
                hit(2, 0, 1),
                hit(3, 2, 3),
                hit(4, 0, 1),
                // The emoji counts two UTF-16 code units, as the webview counts it.
                hit(6, 3, 4),
            ],
            "the header's `a.py` and the `@@` line's `def x` aren't code"
        );
        assert!(!found.more);
    }

    #[test]
    fn matches_dont_overlap_and_a_letter_with_an_accent_matches_its_capital() {
        let patch = "diff --git a/a.py b/a.py\n@@ -0,0 +1 @@\n+aaaa ää İ\n";
        let diff = Diff::parse("1\t0\ta.py\0", patch).unwrap();

        assert_eq!(diff.find("AA").matches, [hit(2, 0, 2), hit(2, 2, 4)]);
        assert_eq!(diff.find("ÄÄ").matches, [hit(2, 5, 7)]);
        // `İ` is two characters in lower case, so it matches only itself, keeping its length in the row.
        assert_eq!(diff.find("İ").matches, [hit(2, 8, 9)]);
    }

    #[test]
    fn an_empty_query_finds_nothing() {
        assert_eq!(
            diff().find(""),
            Found {
                matches: Vec::new(),
                more: false
            }
        );
    }

    #[test]
    fn a_search_stops_at_the_most_matches_and_says_there_are_more() {
        let line = format!("+{}\n", "a".repeat(100));
        let lines = MOST_MATCHES / 100 + 1;
        let patch = format!(
            "diff --git a/a.py b/a.py\n@@ -0,0 +1,{lines} @@\n{}",
            line.repeat(lines)
        );
        let diff = Diff::parse(&format!("{lines}\t0\ta.py\0"), &patch).unwrap();

        let found = diff.find("a");

        assert_eq!(found.matches.len(), MOST_MATCHES);
        assert!(found.more);
        assert!(!diff.find(&"a".repeat(100)).more);
    }
}
