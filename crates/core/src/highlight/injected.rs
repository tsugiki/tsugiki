//! Code in one language inside another (#179): a Markdown code fence, an HTML `<script>`, a reST
//! `code-block`. A language's injections query says where such code is and what it's in, the way
//! tree-sitter's own highlighter reads it, and each piece is parsed as a layer of its own over the file's
//! bytes, so its nodes have the file's offsets.

use std::collections::BTreeMap;
use std::ops::Range;

use tree_sitter::{Node, Parser, Point, Query, QueryCursor, StreamingIterator};

use super::{Language, Layer, Parsed};

/// The most layers code can be inside: a Markdown file's paragraph, an HTML tag in it, a `<script>` in that.
const DEPTH: usize = 4;

/// A language's injections query, with the numbers of the captures it reads.
pub(super) struct Injections {
    pub(super) query: Query,
    /// `@injection.content`: the code in another language.
    content: u32,
    /// `@injection.language`: the node whose text names the language, where the query has one. A pattern
    /// may name it with `#set! injection.language` instead.
    language: Option<u32>,
}

impl Injections {
    /// `query`'s captures, or `None`, logged, if it has no `@injection.content`. A pattern that names a
    /// language Tsugiki doesn't highlight, such as Helix's `comment` for every C comment, is switched off,
    /// as its matches would only cost time.
    pub(super) fn new(mut query: Query) -> Option<Self> {
        let Some(content) = query.capture_index_for_name("injection.content") else {
            log::error!("Tsugiki can't read an injections query with no @injection.content");
            return None;
        };
        let language = query.capture_index_for_name("injection.language");
        for pattern in 0..query.pattern_count() {
            let settings = query.property_settings(pattern);
            let unknown = settings.iter().any(|setting| {
                &*setting.key == "injection.language"
                    && setting.value.as_deref().and_then(Language::named).is_none()
            });
            if unknown {
                query.disable_pattern(pattern);
            }
        }
        Some(Self {
            query,
            content,
            language,
        })
    }
}

/// Whether the code to parse in a captured node takes in the nodes inside it, as a pattern's
/// `#set! injection.include-children` says. A Markdown fence in a quote leaves out the `>` inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Children {
    Included,
    Left,
}

/// Which code a piece of injected code is: that of one node, or that of all the nodes a pattern with
/// `#set! injection.combined` finds in a layer, which parse as one, so that an HTML tag in one part of a
/// Markdown paragraph closes the tag another part opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Piece {
    Node { start: usize, end: usize },
    Combined { pattern: usize, language: usize },
}

/// Code in one language inside a layer: where it is, and which pattern found it.
struct Found {
    language: Language,
    pattern: usize,
    ranges: Vec<Range<usize>>,
}

impl Parsed {
    /// Parses the code in other languages inside each layer, and adds it as a layer after the layers so far.
    pub(super) fn inject(&mut self, parser: &mut Parser) {
        let mut next = 0;
        while let Some(layer) = self.layers.get(next) {
            next += 1;
            if layer.depth >= DEPTH {
                continue;
            }
            let depth = layer.depth + 1;
            for found in self.found(layer) {
                if let Some(layer) = self.parse_inside(parser, found, depth) {
                    self.layers.push(layer);
                }
            }
        }
    }

    /// The code in other languages that `layer`'s injections query finds, in the order it comes. Where
    /// several patterns find the same node, the last that names a language Tsugiki highlights wins.
    fn found(&self, layer: &Layer) -> Vec<Found> {
        let Some(injections) = &layer.grammar.injections else {
            return Vec::new();
        };
        let mut cursor = QueryCursor::new();
        let root = layer.tree.root_node();
        let source = self.source.as_bytes();
        let start = layer.ranges.first().map_or(0, |range| range.start);
        let end = layer.ranges.last().map_or(0, |range| range.end);
        let mut matches =
            (cursor.set_byte_range(start..end)).matches(&injections.query, root, source);
        let mut found: BTreeMap<Piece, Found> = BTreeMap::new();
        while let Some(found_match) = matches.next() {
            let pattern = found_match.pattern_index;
            let Some(language) = self.language(injections, found_match) else {
                continue;
            };
            let settings = injections.query.property_settings(pattern);
            let set = |key: &str| settings.iter().any(|setting| &*setting.key == key);
            let children = if set("injection.include-children") {
                Children::Included
            } else {
                Children::Left
            };
            let combined = set("injection.combined");
            let content = found_match
                .captures()
                .iter()
                .filter(|capture| capture.index == injections.content);
            for capture in content {
                let node = capture.node;
                if combined {
                    let piece = Piece::Combined {
                        pattern,
                        language: language.index,
                    };
                    let kept = found.entry(piece).or_insert_with(|| Found {
                        language,
                        pattern,
                        ranges: Vec::new(),
                    });
                    kept.ranges.extend(self.ranges(node, children));
                    continue;
                }
                let piece = Piece::Node {
                    start: node.start_byte(),
                    end: node.end_byte(),
                };
                if found.get(&piece).is_none_or(|kept| kept.pattern < pattern) {
                    let ranges = self.ranges(node, children);
                    let kept = Found {
                        language,
                        pattern,
                        ranges,
                    };
                    drop(found.insert(piece, kept));
                }
            }
        }
        found.into_values().collect()
    }

    /// The language a match of an injections query names: the text of its `@injection.language`, or its
    /// pattern's `#set! injection.language`.
    fn language(
        &self,
        injections: &Injections,
        found: &tree_sitter::QueryMatch<'_, '_>,
    ) -> Option<Language> {
        let named = found
            .captures()
            .iter()
            .find(|capture| Some(capture.index) == injections.language);
        if let Some(capture) = named {
            return Language::named(self.source.get(capture.node.byte_range())?);
        }
        let settings = injections.query.property_settings(found.pattern_index);
        let set = settings
            .iter()
            .find(|setting| &*setting.key == "injection.language")?;
        Language::named(set.value.as_deref()?)
    }

    /// The bytes of `node` to parse in its language: without those of its named children, unless the query
    /// says to include them, and without the indent its first line starts at on each line after that, so
    /// code in an indented reST directive parses as it would at the start of a line.
    fn ranges(&self, node: Node<'_>, children: Children) -> Vec<Range<usize>> {
        let mut pieces = Vec::new();
        let mut at = node.start_byte();
        if children == Children::Left {
            let mut walk = node.walk();
            for child in node.named_children(&mut walk) {
                pieces.push(at..child.start_byte());
                at = child.end_byte();
            }
        }
        pieces.push(at..node.end_byte());
        let indent = node.start_position().column;
        let mut ranges = Vec::new();
        for piece in pieces {
            let mut from = piece.start;
            while from < piece.end {
                let line_end = self.line_end(from).min(piece.end);
                ranges.push(from..line_end);
                let next_line = self.source.get(line_end..piece.end).unwrap_or("");
                let skipped = next_line
                    .bytes()
                    .take(indent)
                    .take_while(|byte| *byte == b' ' || *byte == b'\t')
                    .count();
                from = line_end + skipped;
            }
        }
        ranges.retain(|range| !range.is_empty());
        ranges
    }

    /// Where the line with byte `byte` ends, after its newline.
    fn line_end(&self, byte: usize) -> usize {
        let next = self.lines.partition_point(|&start| start <= byte);
        self.lines.get(next).copied().unwrap_or(self.source.len())
    }

    /// `found` parsed in its language, as a layer `depth` layers in.
    fn parse_inside(&self, parser: &mut Parser, mut found: Found, depth: usize) -> Option<Layer> {
        let grammar = found.language.grammar()?;
        // No ranges would parse the whole file.
        let _first = found.ranges.first()?;
        // A combined piece's nodes come as the query matched them, which isn't always in order.
        found.ranges.sort_unstable_by_key(|range| range.start);
        let ranges: Vec<tree_sitter::Range> = found
            .ranges
            .iter()
            .map(|range| tree_sitter::Range {
                start_byte: range.start,
                end_byte: range.end,
                start_point: self.point(range.start),
                end_point: self.point(range.end),
            })
            .collect();
        parser.set_language(&grammar.language).ok()?;
        parser.set_included_ranges(&ranges).ok()?;
        let tree = parser.parse(&self.source, None)?;
        Some(Layer {
            grammar,
            tree,
            ranges: found.ranges,
            depth,
        })
    }

    /// The row and column of byte `byte`, as tree-sitter counts them: in lines and bytes from 0.
    fn point(&self, byte: usize) -> Point {
        let row = self
            .lines
            .partition_point(|&start| start <= byte)
            .saturating_sub(1);
        let start = self.lines.get(row).copied().unwrap_or(0);
        Point {
            row,
            column: byte - start,
        }
    }
}
