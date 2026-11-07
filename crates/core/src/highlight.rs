//! Syntax highlighting for the diff (ADR 0001, #153). Each version of a file is parsed whole once with
//! tree-sitter, and the lines a chunk shows are highlighted by running the language's highlights query over
//! those lines' bytes only, so a chunk costs about a millisecond whatever the file's length.
//!
//! Code in one language inside another, such as a Markdown code fence or an HTML `<script>`, is parsed as
//! a layer of its own, and its captures paint over the file's (#179). The languages are a table in
//! `languages.rs`, and `docs/highlighting.md` says how to add one.

mod injected;
mod languages;

use std::cmp::Reverse;
use std::fmt;
use std::ops::{ControlFlow, Range};
use std::sync::{LazyLock, OnceLock};

use serde::Serialize;
use specta::Type;
use tree_sitter::{ParseOptions, ParseState, Parser, Query, QueryCursor, StreamingIterator, Tree};
use tree_sitter_language::LanguageFn;

use languages::LANGUAGES;

/// What Tsugiki knows of a language: an entry in the table in `languages.rs`.
struct Spec {
    /// What a code fence or an injection query calls the language, in lower case.
    name: &'static str,
    /// The other names a code fence or a reST `code-block` may give it, in lower case. A name that's one
    /// of the extensions in `files`, such as `py`, needs no entry here.
    aliases: &'static [&'static str],
    /// The files in the language.
    files: &'static [File],
    /// The grammar, as its crate exports it.
    grammar: LanguageFn,
    /// The highlights queries, read as one in this order. Of the patterns that capture one node, the last
    /// wins, so a query that adds to another's comes after it.
    highlights: &'static [&'static str],
    /// The injections queries, read as one, which say where code in another language is.
    injections: &'static [&'static str],
    /// What some captures paint in this language, where it isn't what `Paint::shared` says: YAML's
    /// `@property` is a key, which github.com shows green. `None` leaves the capture unpainted.
    paints: &'static [(&'static str, Option<Highlight>)],
}

/// Files in a language: those with an extension, such as `Extension("rs")`, or with a whole name, such as
/// `Name("Makefile")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum File {
    Extension(&'static str),
    Name(&'static str),
}

impl File {
    /// Whether the file named `name` is one of these.
    fn holds(self, name: &str) -> bool {
        match self {
            Self::Extension(extension) => name
                .strip_suffix(extension)
                .and_then(|stem| stem.strip_suffix('.'))
                .is_some_and(|stem| !stem.is_empty()),
            Self::Name(whole) => name == whole,
        }
    }
}

/// A language Tsugiki highlights: an entry in the table, and its place there.
#[derive(Clone, Copy)]
pub struct Language {
    spec: &'static Spec,
    index: usize,
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl Eq for Language {}

impl fmt::Debug for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.spec.name)
    }
}

impl Language {
    /// The first language in the table that `wanted` picks.
    fn find(wanted: impl Fn(&Spec) -> bool) -> Option<Self> {
        let (index, spec) = LANGUAGES
            .iter()
            .enumerate()
            .find(|(_, spec)| wanted(spec))?;
        Some(Self { spec, index })
    }

    /// The language of the file at `path`, by its name or extension, if Tsugiki highlights it.
    pub fn of(path: &str) -> Option<Self> {
        let name = path.rsplit('/').next().unwrap_or(path);
        Self::find(|spec| spec.files.iter().any(|file| file.holds(name)))
    }

    /// The language a code fence or an injection query names, such as `python`, `py` or `Python3`, if
    /// Tsugiki highlights it.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
        let called = |other: &str| other.eq_ignore_ascii_case(name);
        let extension =
            |file: &File| matches!(*file, File::Extension(extension) if called(extension));
        Self::find(|spec| {
            called(spec.name)
                || spec.aliases.iter().any(|alias| called(alias))
                || spec.files.iter().any(extension)
        })
    }

    /// The language's grammar and queries, compiled the first time a file in it is parsed. A query that
    /// doesn't compile leaves the language plain; a test compiles each one.
    fn grammar(self) -> Option<&'static Grammar> {
        static GRAMMARS: LazyLock<Vec<OnceLock<Option<Grammar>>>> =
            LazyLock::new(|| LANGUAGES.iter().map(|_| OnceLock::new()).collect());
        let compiled = GRAMMARS.get(self.index)?;
        compiled
            .get_or_init(|| Grammar::compile(self.spec))
            .as_ref()
    }
}

/// What a run of text shows as, in colors and styles that look like github.com's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Highlight {
    Keyword,
    String,
    Comment,
    Constant,
    Function,
    Type,
    /// An HTML tag's name, or a key in YAML or TOML.
    Tag,
    /// A heading in Markdown or reST.
    Heading,
    Strong,
    Emphasis,
}

/// What a capture paints: a color, or the plain text of code inside a string, such as an f-string's `{x}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Plain,
    Color(Highlight),
}

impl Paint {
    /// What a highlights query's capture paints in a language with the overrides `paints`, or `None` for one
    /// Tsugiki leaves alone, such as `@variable` or `@punctuation.delimiter`. The longest leading part of
    /// the name that has a rule decides: `@function.builtin` paints as `function` does.
    fn of(capture: &str, paints: &[(&str, Option<Highlight>)]) -> Option<Self> {
        let mut part = capture;
        loop {
            if let Some((_, highlight)) = paints.iter().find(|(name, _)| *name == part) {
                return highlight.map(Self::Color);
            }
            if let Some(paint) = Self::shared(part) {
                return paint;
            }
            part = part.rsplit_once('.')?.0;
        }
    }

    /// The rule for a capture name, or its leading part, in every language: `Some(None)` leaves it alone,
    /// and `None` means there's no rule. The names are those of Helix's, Neovim's and the grammars' own
    /// queries.
    fn shared(name: &str) -> Option<Option<Self>> {
        let highlight = match name {
            "keyword" | "operator" | "conditional" | "repeat" | "include" | "exception"
            | "type.qualifier" => Highlight::Keyword,
            "string" | "character" | "constant.character" | "markup.link" | "text.uri"
            | "text.reference" => Highlight::String,
            "comment" => Highlight::Comment,
            "constant"
            | "number"
            | "float"
            | "boolean"
            | "escape"
            | "string.escape"
            | "string.special.key"
            | "string.special.symbol"
            | "constant.character.escape"
            | "markup.raw"
            | "text.literal" => Highlight::Constant,
            "function" | "method" | "attribute" | "decorator" | "tag.attribute" => {
                Highlight::Function
            }
            "type" | "constructor" => Highlight::Type,
            "tag" => Highlight::Tag,
            "markup.heading" | "text.title" => Highlight::Heading,
            "markup.bold" | "text.strong" => Highlight::Strong,
            "markup.italic" | "text.emphasis" => Highlight::Emphasis,
            "embedded" | "none" => return Some(Some(Self::Plain)),
            "tag.delimiter" => return Some(None),
            _ => return None,
        };
        Some(Some(Self::Color(highlight)))
    }
}

/// What one of a highlights query's captures says of the node it captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    /// The node shows as this paints it, or, with `None`, as the nodes around it show: `@variable`. Of the
    /// patterns that capture one node, the last decides, even when it leaves the node unpainted.
    Shows(Option<Paint>),
    /// A capture whose name starts with `_`, which only a predicate reads: `@_type`.
    Predicate,
}

/// A language's grammar and queries, with what each of the highlights query's captures says.
struct Grammar {
    language: tree_sitter::Language,
    highlights: Query,
    captures: Vec<Capture>,
    injections: Option<injected::Injections>,
}

impl Grammar {
    /// The grammar of `spec`, with its queries compiled, or `None`, logged, if one doesn't compile.
    fn compile(spec: &Spec) -> Option<Self> {
        let language = tree_sitter::Language::new(spec.grammar);
        let highlights = compile(spec, &language, spec.highlights)?;
        let captures = highlights
            .capture_names()
            .iter()
            .map(|name| match name.strip_prefix('_') {
                Some(_) => Capture::Predicate,
                None => Capture::Shows(Paint::of(name, spec.paints)),
            })
            .collect();
        let injections = match spec.injections {
            [] => None,
            queries => Some(injected::Injections::new(compile(
                spec, &language, queries,
            )?)?),
        };
        Some(Self {
            language,
            highlights,
            captures,
            injections,
        })
    }
}

/// `queries` read as one query of `spec`'s language, or `None`, logged, if it doesn't compile.
fn compile(spec: &Spec, language: &tree_sitter::Language, queries: &[&str]) -> Option<Query> {
    match Query::new(language, &queries.join("\n")) {
        Ok(query) => Some(query),
        Err(error) => {
            let name = spec.name;
            log::error!(
                "Tsugiki can't compile a query of {name}'s, so it shows {name} plain: {error}"
            );
            None
        }
    }
}

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
    /// The file's own tree first, then the trees of the code in other languages inside it, each after the
    /// layer it's inside.
    layers: Vec<Layer>,
}

/// The tree of a file, or of the code in one language inside it.
struct Layer {
    grammar: &'static Grammar,
    tree: Tree,
    /// The bytes of the file the tree was parsed from, in order. A node of an inner layer can span bytes
    /// left out between them, such as the `>` of a quote around a Markdown fence, which it doesn't paint.
    ranges: Vec<Range<usize>>,
    /// How many layers it's inside: none for the file's own.
    depth: usize,
}

impl fmt::Debug for Parsed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Parsed")
            .field("language", &self.language)
            .field("lines", &self.lines.len())
            .field("layers", &self.layers.len())
            .finish_non_exhaustive()
    }
}

/// What a parse that was asked to stop gives: nothing, since it threw away what it had parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

/// The bytes a tree takes for each node `Node::descendant_count` counts. A counting allocator measured 78 to
/// 112 bytes on CPython's largest C and Python files (`Objects/typeobject.c` took 7.2 MB, and
/// `Lib/test/test_typing.py` 13.3 MB), and this adds what the allocator keeps beside each block (#180).
const BYTES_PER_NODE: usize = 128;

impl Parsed {
    /// Parses `source` whole, and the code in other languages inside it. Code tree-sitter can't make sense
    /// of, such as a macro the C grammar doesn't know, still parses, with an error node around it.
    pub fn parse(language: Language, source: String) -> Option<Self> {
        Self::parse_unless(language, source, &mut || false).unwrap_or(None)
    }

    /// Parses as `parse` does, unless `stop` says to stop. It asks every hundred or so steps of the
    /// parse, and before each piece of code inside the file.
    pub fn parse_unless(
        language: Language,
        source: String,
        stop: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Self>, Stopped> {
        let mut stopped = false;
        let parsed = Self::parse_while(language, source, &mut || {
            stopped = stop();
            !stopped
        });
        if stopped { Err(Stopped) } else { Ok(parsed) }
    }

    /// Parses as `parse` does, while `go_on` says to.
    fn parse_while(
        language: Language,
        source: String,
        go_on: &mut dyn FnMut() -> bool,
    ) -> Option<Self> {
        let grammar = language.grammar()?;
        let mut parser = Parser::new();
        parser.set_language(&grammar.language).ok()?;
        let mut progress = |_: &ParseState| {
            if go_on() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        };
        let options = ParseOptions::new().progress_callback(&mut progress);
        let bytes = source.as_bytes();
        let read = &mut |at: usize, _| bytes.get(at..).unwrap_or_default();
        let tree = parser.parse_with_options(read, None, Some(options))?;
        let ends = source.match_indices('\n').map(|(at, _)| at + 1);
        let lines = std::iter::once(0).chain(ends).collect();
        let whole = Layer {
            grammar,
            tree,
            ranges: std::iter::once(0..source.len()).collect(),
            depth: 0,
        };
        let mut parsed = Self {
            language,
            source,
            lines,
            layers: vec![whole],
        };
        parsed.inject(&mut parser, go_on)?;
        Some(parsed)
    }

    /// About how many bytes of memory this takes: the file, where its lines start, and its trees.
    pub fn cost(&self) -> usize {
        let nodes: usize = self
            .layers
            .iter()
            .map(|layer| layer.tree.root_node().descendant_count())
            .sum();
        self.source.len() + self.lines.len() * size_of::<usize>() + nodes * BYTES_PER_NODE
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

    /// What paints each byte of `bytes`: each layer over them in turn, so code inside another language
    /// paints over the code around it.
    fn paint(&self, bytes: Range<usize>) -> Vec<Option<Paint>> {
        let mut canvas = Canvas {
            paint: vec![None; bytes.len()],
            bytes,
        };
        let mut cursor = QueryCursor::new();
        for layer in &self.layers {
            if layer.within(&canvas.bytes).next().is_some() {
                layer.paint(&mut cursor, &self.source, &mut canvas);
            }
        }
        canvas.paint
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

/// What paints each byte of a run of the file's bytes.
struct Canvas {
    bytes: Range<usize>,
    paint: Vec<Option<Paint>>,
}

impl Layer {
    /// The parts of `bytes` the layer was parsed from.
    fn within(&self, bytes: &Range<usize>) -> impl Iterator<Item = Range<usize>> {
        let (start, end) = (bytes.start, bytes.end);
        let first = self.ranges.partition_point(|range| range.end <= start);
        let after = self.ranges.get(first..).unwrap_or_default();
        after
            .iter()
            .take_while(move |range| range.start < end)
            .map(move |range| range.start.max(start)..range.end.min(end))
            .filter(|range| !range.is_empty())
    }

    /// Paints `canvas`, which covers bytes of `source`, with this layer's captures over them. Where
    /// captures overlap, the innermost node paints, and of the patterns that capture one node, the last in
    /// the query decides, as in Helix and Neovim. The queries rely on it: they list `(identifier) @variable`
    /// first, and the patterns for more particular names after it.
    fn paint(&self, cursor: &mut QueryCursor, source: &str, canvas: &mut Canvas) {
        let bytes = canvas.bytes.clone();
        let grammar = self.grammar;
        let root = self.tree.root_node();
        let mut captures = (cursor.set_byte_range(bytes.clone())).captures(
            &grammar.highlights,
            root,
            source.as_bytes(),
        );
        let mut found = Vec::new();
        while let Some((found_match, index)) = captures.next() {
            let capture = found_match.captures().get(*index);
            let at = capture.map(|capture| usize::try_from(capture.index).unwrap_or(usize::MAX));
            if let Some(capture) = capture
                && let Some(Capture::Shows(shows)) = at.and_then(|at| grammar.captures.get(at))
            {
                let node = capture.node;
                found.push((
                    node.start_byte(),
                    node.end_byte(),
                    found_match.pattern_index,
                    *shows,
                ));
            }
        }
        // Outer nodes before the nodes inside them, so the inner ones paint over them.
        found.sort_by_key(|&(start, end, pattern, _)| (start, Reverse(end), Reverse(pattern)));
        found.dedup_by_key(|&mut (start, end, _, _)| (start, end));
        let painted = found
            .into_iter()
            .filter_map(|(start, end, _, shows)| Some((start..end, shows?)));
        for (node, shows) in painted {
            for part in self.within(&node) {
                let part = part.start.max(bytes.start)..part.end.min(bytes.end);
                let run =
                    part.start.saturating_sub(bytes.start)..part.end.saturating_sub(bytes.start);
                if let Some(run) = canvas.paint.get_mut(run) {
                    run.fill(Some(shows));
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
