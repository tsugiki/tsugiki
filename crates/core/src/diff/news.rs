//! A PR's **NEWS entries**, rendered for the top of its Files tab. An entry is reStructuredText that
//! CPython's docs build renders with Sphinx. This renders the little of it entries use: paragraphs, lists,
//! code, inline markup, links and Sphinx's roles, from the tree tree-sitter's grammar parses (#181).

use std::collections::HashMap;

use serde::Serialize;
use specta::Type;
use tree_sitter::{Node, Parser};

use crate::web::WebAddress;

/// Where CPython keeps the entries of the next release, one folder per section.
const NEXT: &str = "Misc/NEWS.d/next/";

/// A NEWS entry the PR adds or changes, as its head has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NewsEntry {
    /// Its path in the diff.
    pub path: String,
    /// The changelog's section, as Sphinx titles it: "Core and Builtins" from `Core_and_Builtins`.
    pub section: String,
    /// The issue its file name gives.
    pub issue: Option<NewsLink>,
    pub blocks: Vec<NewsBlock>,
}

/// A link's text and what it goes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct NewsLink {
    pub text: String,
    pub to: LinkTo,
}

/// What a link goes to. The window knows the address of each kind, as CPython's docs build does, so the core
/// names no site but GitHub (ADR 0005).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "site", rename_all = "snake_case")]
pub enum LinkTo {
    /// A web page the entry gives the address of.
    Web {
        url: WebAddress,
    },
    Pep {
        number: u32,
    },
    /// An issue in CPython's GitHub repository.
    GhIssue {
        number: u32,
    },
    /// An issue in bugs.python.org, CPython's tracker before GitHub.
    Bpo {
        number: u32,
    },
    Rfc {
        number: u32,
    },
    Cve {
        id: String,
    },
    /// A project on the Python Package Index.
    Pypi {
        project: String,
    },
}

/// A paragraph, a list or a block of code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewsBlock {
    Paragraph {
        pieces: Vec<NewsPiece>,
    },
    /// Each item holds blocks of its own.
    List {
        items: Vec<Vec<NewsBlock>>,
    },
    Code {
        text: String,
    },
}

/// A run of a paragraph's text, all shown one way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewsPiece {
    Text { text: String },
    Emphasis { text: String },
    Strong { text: String },
    Code { text: String },
    Link { text: String, to: LinkTo },
}

impl NewsPiece {
    /// Whether the piece shows anything: a text left empty doesn't.
    fn shows(&self) -> bool {
        !matches!(self, Self::Text { text } if text.is_empty())
    }
}

impl NewsEntry {
    /// Whether the file at `path` is a NEWS entry: `Misc/NEWS.d/next/<section>/<name>.rst`.
    pub fn is_entry(path: &str) -> bool {
        path.strip_prefix(NEXT)
            .is_some_and(|rest| rest.contains('/') && path.ends_with(".rst"))
    }

    /// The entry at `path`, rendered from its text `source`.
    pub fn render(path: &str, source: &str) -> Self {
        let rest = path.strip_prefix(NEXT).unwrap_or(path);
        let (folder, name) = rest.split_once('/').unwrap_or(("", rest));
        Self {
            path: path.to_owned(),
            section: folder.replace('_', " ").replace('-', "/"),
            issue: name.split('.').find_map(issue),
            blocks: blocks(source).unwrap_or_default(),
        }
    }
}

/// The issue a part of an entry's file name names: `gh-issue-123` on GitHub, or `bpo-123` on bugs.python.org.
fn issue(part: &str) -> Option<NewsLink> {
    let (text, to) = if let Some(number) = part.strip_prefix("gh-issue-") {
        let number: u32 = number.parse().ok()?;
        (format!("gh-{number}"), LinkTo::GhIssue { number })
    } else {
        let number: u32 = part.strip_prefix("bpo-")?.parse().ok()?;
        (format!("bpo-{number}"), LinkTo::Bpo { number })
    };
    Some(NewsLink { text, to })
}

/// The blocks of an entry, or `None` if the grammar can't load.
fn blocks(source: &str) -> Option<Vec<NewsBlock>> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rst::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let blocks = Blocks {
        source,
        targets: targets(root, source),
    };
    Some(blocks.of(root))
}

/// The address of each hyperlink target in the entry, `.. _name: address`, by its name as `named` gives it.
fn targets(root: Node<'_>, source: &str) -> HashMap<String, String> {
    let text = |node: Option<Node<'_>>| node.and_then(|node| source.get(node.byte_range()));
    let mut walk = root.walk();
    root.named_children(&mut walk)
        .filter(|child| child.kind() == "target")
        .filter_map(|target| {
            let name = text(target.child_by_field_name("name"))?;
            let link = text(target.child_by_field_name("link"))?;
            let name = name.strip_prefix('_').unwrap_or(name).trim_end_matches(':');
            Some((named(name), link.to_owned()))
        })
        .collect()
}

/// A reference name as reStructuredText matches it: without its backticks, its case or runs of space.
fn named(name: &str) -> String {
    joined(name.trim_matches('`')).trim().to_lowercase()
}

/// A paragraph of `text` as it stands, its lines joined.
fn plain(text: &str) -> NewsBlock {
    NewsBlock::Paragraph {
        pieces: vec![NewsPiece::Text {
            text: joined(text).trim().to_owned(),
        }],
    }
}

struct Blocks<'a> {
    source: &'a str,
    targets: HashMap<String, String>,
}

impl Blocks<'_> {
    /// The blocks of the children of `node`.
    fn of(&self, node: Node<'_>) -> Vec<NewsBlock> {
        let mut blocks = Vec::new();
        let mut walk = node.walk();
        for child in node.named_children(&mut walk) {
            match child.kind() {
                // A target names an address for references, and Sphinx doesn't show it.
                "comment" | "target" => {}
                "paragraph" => self.paragraph(child, &mut blocks),
                "bullet_list" | "enumerated_list" => blocks.push(self.list(child)),
                "literal_block" => blocks.push(self.code(child)),
                _ => blocks.push(plain(self.text(child))),
            }
        }
        blocks
    }

    /// Each item's blocks, from the `body` it holds.
    fn list(&self, node: Node<'_>) -> NewsBlock {
        let mut walk = node.walk();
        let items = node
            .named_children(&mut walk)
            .map(|item| {
                let mut walk = item.walk();
                let body = item
                    .named_children(&mut walk)
                    .find(|child| child.kind() == "body");
                body.map_or_else(Vec::new, |body| self.of(body))
            })
            .collect();
        NewsBlock::List { items }
    }

    /// A paragraph's pieces, and the code it ends with after `::`. As in Sphinx, `text::` shows as `text:`,
    /// `text ::` as `text`, and a lone `::` as nothing.
    fn paragraph(&self, node: Node<'_>, blocks: &mut Vec<NewsBlock>) {
        let mut pieces = Vec::new();
        let mut code = None;
        let mut at = node.start_byte();
        let mut walk = node.walk();
        for child in node.named_children(&mut walk) {
            if child.kind() == "literal_block" {
                let text = before_code(&self.gap(at, child.start_byte()));
                pieces.push(NewsPiece::Text { text });
                code = Some(self.code(child));
                break;
            }
            self.between(at, child.start_byte(), &mut pieces);
            at = child.end_byte();
            pieces.push(self.inline(child));
        }
        if code.is_none() {
            self.between(at, node.end_byte(), &mut pieces);
        }
        trim(&mut pieces);
        if !pieces.is_empty() {
            blocks.push(NewsBlock::Paragraph { pieces });
        }
        blocks.extend(code);
    }

    /// The text from `start` to `end`, unescaped and its lines joined.
    fn gap(&self, start: usize, end: usize) -> String {
        joined(&unescaped(self.source.get(start..end).unwrap_or("")))
    }

    /// The text between two pieces, if there is any.
    fn between(&self, start: usize, end: usize, pieces: &mut Vec<NewsPiece>) {
        let text = self.gap(start, end);
        if !text.is_empty() {
            pieces.push(NewsPiece::Text { text });
        }
    }

    /// A block of code, its lines moved left as far as they all go.
    fn code(&self, node: Node<'_>) -> NewsBlock {
        let line_start = self
            .source
            .get(..node.start_byte())
            .and_then(|before| before.rfind('\n'))
            .map_or(0, |newline| newline + 1);
        let text = self.source.get(line_start..node.end_byte()).unwrap_or("");
        let indent = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let lines: Vec<&str> = text
            .lines()
            .map(|line| line.get(indent..).unwrap_or(""))
            .collect();
        NewsBlock::Code {
            text: lines.join("\n").trim_end().to_owned(),
        }
    }

    /// One piece of inline markup.
    fn inline(&self, node: Node<'_>) -> NewsPiece {
        let text = self.text(node);
        let inside = |marks: &str| {
            let inner = text.strip_prefix(marks).and_then(|t| t.strip_suffix(marks));
            joined(inner.unwrap_or(text))
        };
        match node.kind() {
            "emphasis" => NewsPiece::Emphasis { text: inside("*") },
            "strong" => NewsPiece::Strong { text: inside("**") },
            "literal" => NewsPiece::Code { text: inside("``") },
            "interpreted_text" => self.interpreted(node),
            "reference" => self.reference(text),
            "standalone_hyperlink" => web(text, text),
            _ => NewsPiece::Text { text: joined(text) },
        }
    }

    /// `` :role:`content` ``, shown as Sphinx shows that role. Text in backticks with no role is a title,
    /// which Sphinx sets in italics.
    fn interpreted(&self, node: Node<'_>) -> NewsPiece {
        let text = self.text(node);
        let mut walk = node.walk();
        let role = node
            .named_children(&mut walk)
            .find(|child| child.kind() == "role");
        let Some(role) = role else {
            return NewsPiece::Emphasis {
                text: joined(text.trim_matches('`')),
            };
        };
        let content = if role.start_byte() == node.start_byte() {
            self.source.get(role.end_byte()..node.end_byte())
        } else {
            self.source.get(node.start_byte()..role.start_byte())
        };
        let name = self.text(role).trim_matches(':');
        shown_role(name, &joined(content.unwrap_or("").trim_matches('`')))
    }

    /// `` `text <url>`_ `` as a link, or `` `name`_ `` as a link to the target of that name. A name with no
    /// target is only its text.
    fn reference(&self, text: &str) -> NewsPiece {
        let inner = joined(text.trim_end_matches('_').trim_matches('`'));
        match titled(&inner) {
            (Some(title), url) => web(title, url),
            (None, name) => match self.targets.get(&named(name)) {
                Some(url) => web(name, url),
                None => NewsPiece::Text {
                    text: name.to_owned(),
                },
            },
        }
    }

    fn text(&self, node: Node<'_>) -> &str {
        self.source.get(node.byte_range()).unwrap_or("")
    }
}

/// `content` of the role `name`, as CPython's docs show it.
fn shown_role(name: &str, content: &str) -> NewsPiece {
    let (title, target) = titled(content);
    let shown = title.unwrap_or(target);
    let number = target.split('#').next().unwrap_or("").parse::<u32>().ok();
    let link = match (name, number) {
        ("pep", Some(number)) => Some((format!("PEP {number}"), LinkTo::Pep { number })),
        ("gh", Some(number)) => Some((format!("gh-{number}"), LinkTo::GhIssue { number })),
        ("issue", Some(number)) => Some((format!("bpo-{number}"), LinkTo::Bpo { number })),
        ("rfc", Some(number)) => Some((format!("RFC {number}"), LinkTo::Rfc { number })),
        ("cve", _) => Some((
            target.to_owned(),
            LinkTo::Cve {
                id: target.to_owned(),
            },
        )),
        ("pypi", _) => Some((
            target.to_owned(),
            LinkTo::Pypi {
                project: target.to_owned(),
            },
        )),
        _ => None,
    };
    if let Some((text, to)) = link {
        return NewsPiece::Link {
            text: title.map_or(text, str::to_owned),
            to,
        };
    }
    match name {
        // A number that doesn't read as one leaves nothing to link to.
        "pep" | "gh" | "issue" | "rfc" | "term" | "ref" | "doc" => NewsPiece::Text {
            text: shown.to_owned(),
        },
        "program" => NewsPiece::Strong {
            text: shown.to_owned(),
        },
        _ => NewsPiece::Code {
            text: title.map_or_else(|| object(name, target), str::to_owned),
        },
    }
}

/// An object's name as Sphinx shows it: `~` keeps its last part, `!` only stops the link, and a function
/// gets its parentheses.
fn object(role: &str, target: &str) -> String {
    let target = target.strip_prefix('!').unwrap_or(target);
    let target = match target.strip_prefix('~') {
        Some(full) => full.rsplit('.').next().unwrap_or(full),
        None => target,
    };
    let call = matches!(role, "func" | "meth" | "c:func") && !target.ends_with(')');
    if call {
        format!("{target}()")
    } else {
        target.to_owned()
    }
}

/// The title and target of `title <target>`, or no title.
fn titled(content: &str) -> (Option<&str>, &str) {
    content
        .strip_suffix('>')
        .and_then(|rest| rest.rsplit_once(" <"))
        .map_or((None, content), |(title, target)| {
            (Some(title.trim()), target)
        })
}

/// A link to `url` if it is a web page the browser may open, or else `text` alone: an entry is a stranger's
/// text.
fn web(text: &str, url: &str) -> NewsPiece {
    match WebAddress::parse(url) {
        Ok(url) => NewsPiece::Link {
            text: text.to_owned(),
            to: LinkTo::Web { url },
        },
        Err(_) => NewsPiece::Text {
            text: text.to_owned(),
        },
    }
}

/// `text` with each run of spaces and line ends made one space, as a paragraph's lines join.
fn joined(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// `text` with each backslash escape replaced by the character it escapes. An escaped space goes.
fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => out.extend(chars.next().filter(|next| !next.is_whitespace())),
            (c, false) => out.push(c),
        }
    }
    out
}

/// Trims the space at a paragraph's ends.
fn trim(pieces: &mut Vec<NewsPiece>) {
    if let Some(NewsPiece::Text { text }) = pieces.first_mut() {
        *text = text.trim_start().to_owned();
    }
    if let Some(NewsPiece::Text { text }) = pieces.last_mut() {
        *text = text.trim_end().to_owned();
    }
    pieces.retain(NewsPiece::shows);
}

/// Shows the text that ends a paragraph before code, `::` and all, as Sphinx does.
fn before_code(text: &str) -> String {
    let text = text.trim_end();
    let before = text.strip_suffix("::").unwrap_or(text);
    if before.is_empty() || before.ends_with(' ') {
        before.trim_end().to_owned()
    } else {
        format!("{before}:")
    }
}

#[cfg(test)]
mod tests;
