//! Syntax highlighting for the diff (ADR 0001, #153). Each version of a file is parsed whole once with
//! tree-sitter, and the lines a chunk shows are highlighted by running the language's highlights query over
//! those lines' bytes only, so a chunk costs about a millisecond whatever the file's length.

use std::cmp::Reverse;
use std::fmt;
use std::ops::Range;
use std::sync::LazyLock;

use serde::Serialize;
use specta::Type;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

/// A language Tsugiki highlights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    C,
    Python,
}

impl Language {
    /// The language of the file at `path`, by its extension, if Tsugiki highlights it.
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        match name.rsplit_once('.')?.1 {
            "c" | "h" => Some(Self::C),
            "py" | "pyi" | "pyw" => Some(Self::Python),
            _ => None,
        }
    }

    fn grammar(self) -> &'static Grammar {
        match self {
            Self::C => &C,
            Self::Python => &PYTHON,
        }
    }
}

/// What a run of text shows as, in colors that look like github.com's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Highlight {
    Keyword,
    String,
    Comment,
    Constant,
    Function,
    Type,
}

/// What a capture paints: a color, or the plain text of code inside a string, such as an f-string's `{x}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Plain,
    Color(Highlight),
}

impl Paint {
    /// What a highlights query's capture paints, or `None` for one Tsugiki leaves alone, such as `@variable`
    /// or `@punctuation.delimiter`. The part before the first dot decides: `@function.builtin` is a function.
    fn of(capture: &str) -> Option<Self> {
        let kind = capture.split('.').next().unwrap_or(capture);
        let color = match kind {
            "keyword" | "operator" => Highlight::Keyword,
            "string" => Highlight::String,
            "comment" => Highlight::Comment,
            "number" | "constant" | "escape" => Highlight::Constant,
            "function" | "attribute" => Highlight::Function,
            "type" | "constructor" => Highlight::Type,
            "embedded" => return Some(Self::Plain),
            _ => return None,
        };
        Some(Self::Color(color))
    }
}

/// A language's grammar and its highlights query, with what each of the query's captures paints.
struct Grammar {
    language: tree_sitter::Language,
    query: Query,
    paints: Vec<Option<Paint>>,
}

impl Grammar {
    #[expect(
        clippy::expect_used,
        reason = "the query ships with its grammar's crate, and a test compiles each one"
    )]
    fn new(language: tree_sitter::Language, highlights: &str) -> Self {
        let query =
            Query::new(&language, highlights).expect("a grammar's own highlights query compiles");
        let paints = query
            .capture_names()
            .iter()
            .map(|name| Paint::of(name))
            .collect();
        Self {
            language,
            query,
            paints,
        }
    }
}

static C: LazyLock<Grammar> = LazyLock::new(|| {
    Grammar::new(
        tree_sitter_c::LANGUAGE.into(),
        tree_sitter_c::HIGHLIGHT_QUERY,
    )
});

static PYTHON: LazyLock<Grammar> = LazyLock::new(|| {
    Grammar::new(
        tree_sitter_python::LANGUAGE.into(),
        tree_sitter_python::HIGHLIGHTS_QUERY,
    )
});

/// A run of one line's text in one color, from `start` to `end` in UTF-16 code units, as the webview counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub highlight: Highlight,
}

/// One version of a file, parsed whole.
pub struct Parsed {
    language: Language,
    source: String,
    /// Where each line starts, in bytes.
    lines: Vec<usize>,
    tree: Tree,
}

impl fmt::Debug for Parsed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Parsed")
            .field("language", &self.language)
            .field("lines", &self.lines.len())
            .finish_non_exhaustive()
    }
}

impl Parsed {
    /// Parses `source` whole. Code tree-sitter can't make sense of, such as a macro the C grammar doesn't
    /// know, still parses, with an error node around it.
    pub fn parse(language: Language, source: String) -> Option<Self> {
        let mut parser = Parser::new();
        parser.set_language(&language.grammar().language).ok()?;
        let tree = parser.parse(&source, None)?;
        let ends = source.match_indices('\n').map(|(at, _)| at + 1);
        let lines = std::iter::once(0).chain(ends).collect();
        Some(Self {
            language,
            source,
            lines,
            tree,
        })
    }

    /// The spans of each line in `lines`, numbered from 1 as a diff numbers them. A line past the end has
    /// none. Only the captures over those lines' bytes are read.
    pub fn spans(&self, lines: Range<u32>) -> Vec<Vec<Span>> {
        let start = self.line_start(lines.start);
        let end = self.line_start(lines.end).max(start);
        let paint = self.paint(start..end);
        lines
            .map(|line| self.line_spans(line, start, &paint))
            .collect()
    }

    /// Where line `line` starts, or the end for a line past it.
    fn line_start(&self, line: u32) -> usize {
        let index = usize::try_from(line.saturating_sub(1)).unwrap_or(usize::MAX);
        self.lines.get(index).copied().unwrap_or(self.source.len())
    }

    /// What paints each byte of `bytes`. Where captures overlap, the innermost node paints, and of the
    /// patterns that capture one node and paint it, the last in the query wins, as in tree-sitter's own
    /// highlighter. The grammars' queries rely on it: they list `(identifier) @variable` first, and the
    /// patterns for more particular names after it.
    fn paint(&self, bytes: Range<usize>) -> Vec<Option<Paint>> {
        let mut paint = vec![None; bytes.len()];
        if bytes.is_empty() {
            return paint;
        }
        let grammar = self.language.grammar();
        let mut cursor = QueryCursor::new();
        let root = self.tree.root_node();
        let mut captures = (cursor.set_byte_range(bytes.clone())).captures(
            &grammar.query,
            root,
            self.source.as_bytes(),
        );
        let mut found = Vec::new();
        while let Some((found_match, index)) = captures.next() {
            let capture = found_match.captures().get(*index);
            let at = capture.map(|capture| usize::try_from(capture.index).unwrap_or(usize::MAX));
            if let Some(capture) = capture
                && let Some(Some(painted)) = at.and_then(|at| grammar.paints.get(at))
            {
                let node = capture.node;
                found.push((
                    node.start_byte(),
                    node.end_byte(),
                    found_match.pattern_index,
                    *painted,
                ));
            }
        }
        // Outer nodes before the nodes inside them, so the inner ones paint over them.
        found.sort_by_key(|&(start, end, pattern, _)| (start, Reverse(end), Reverse(pattern)));
        found.dedup_by_key(|&mut (start, end, _, _)| (start, end));
        for (start, end, _, painted) in found {
            let from = start.max(bytes.start) - bytes.start;
            let to = end.min(bytes.end).saturating_sub(bytes.start);
            if let Some(run) = paint.get_mut(from..to) {
                run.fill(Some(painted));
            }
        }
        paint
    }

    /// The spans of line `line`, where `paint` covers the bytes from `start`.
    fn line_spans(&self, line: u32, start: usize, paint: &[Option<Paint>]) -> Vec<Span> {
        let from = self.line_start(line);
        let to = self.line_start(line + 1);
        let text = self.source.get(from..to).unwrap_or("");
        let text = text.strip_suffix('\n').unwrap_or(text);
        let mut spans: Vec<Span> = Vec::new();
        let mut at = 0;
        for (byte, character) in text.char_indices() {
            let width = u32::try_from(character.len_utf16()).unwrap_or(2);
            let painted = paint.get(from + byte - start).copied().flatten();
            if let Some(Paint::Color(highlight)) = painted {
                match spans.last_mut() {
                    Some(last) if last.end == at && last.highlight == highlight => {
                        last.end += width
                    }
                    _ => spans.push(Span {
                        start: at,
                        end: at + width,
                        highlight,
                    }),
                }
            }
            at += width;
        }
        spans
    }
}

#[cfg(test)]
pub(crate) mod tests;
