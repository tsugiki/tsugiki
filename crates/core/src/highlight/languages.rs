//! The languages Tsugiki highlights. To add one, add its grammar's crate to the workspace and to
//! `crates/core/Cargo.toml`, add an entry here, and add a sample file to `samples/`. `docs/highlighting.md`
//! walks through it.

use super::File::{Extension, Name};
use super::{Highlight, Spec};

/// A query Tsugiki keeps in `crates/core/queries`, because its grammar's crate has none, or has one that
/// needs what Tsugiki's highlighter doesn't do.
macro_rules! query {
    ($path:literal) => {
        include_str!(concat!("../../queries/", $path))
    };
}

/// One entry per language, in order of name.
pub(super) static LANGUAGES: &[Spec] = &[
    Spec {
        name: "bash",
        aliases: &["shell", "zsh"],
        files: &[Extension("sh"), Extension("bash")],
        grammar: tree_sitter_bash::LANGUAGE,
        highlights: &[tree_sitter_bash::HIGHLIGHT_QUERY],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "c",
        aliases: &[],
        files: &[Extension("c"), Extension("h")],
        grammar: tree_sitter_c::LANGUAGE,
        highlights: &[query!("c/highlights.scm")],
        injections: &[query!("c/injections.scm")],
        paints: &[],
    },
    Spec {
        name: "css",
        aliases: &[],
        files: &[Extension("css")],
        grammar: tree_sitter_css::LANGUAGE,
        highlights: &[
            tree_sitter_css::HIGHLIGHTS_QUERY,
            // github.com shows a class or an id in a selector as it does a function's name.
            "[(class_name) (id_name)] @function",
        ],
        injections: &[],
        paints: &[("property", Some(Highlight::Constant))],
    },
    Spec {
        name: "html",
        aliases: &["xhtml"],
        files: &[Extension("html"), Extension("htm")],
        grammar: tree_sitter_html::LANGUAGE,
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY, QUOTED_ATTRIBUTE],
        injections: &[tree_sitter_html::INJECTIONS_QUERY],
        paints: &[],
    },
    Spec {
        name: "javascript",
        aliases: &["node"],
        files: &[
            Extension("js"),
            Extension("mjs"),
            Extension("cjs"),
            Extension("jsx"),
        ],
        grammar: tree_sitter_javascript::LANGUAGE,
        highlights: &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        ],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "json",
        aliases: &[],
        files: &[Extension("json")],
        grammar: tree_sitter_json::LANGUAGE,
        highlights: &[
            tree_sitter_json::HIGHLIGHTS_QUERY,
            // The query paints a key before it paints every string, which wins here.
            "(pair key: (_) @string.special.key)",
        ],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "make",
        aliases: &["makefile"],
        files: &[
            Name("Makefile"),
            Name("GNUmakefile"),
            Name("makefile"),
            Name("Makefile.pre.in"),
            Extension("mk"),
            Extension("make"),
        ],
        grammar: tree_sitter_make::LANGUAGE,
        highlights: &[tree_sitter_make::HIGHLIGHTS_QUERY],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "markdown",
        aliases: &[],
        files: &[Extension("md"), Extension("markdown")],
        grammar: tree_sitter_md::LANGUAGE,
        highlights: &[
            tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
            // The `>` of a quote, which a fence's text starts after on its first line.
            "(block_continuation) @none",
        ],
        injections: &[tree_sitter_md::INJECTION_QUERY_BLOCK],
        paints: &[],
    },
    // The text of a Markdown paragraph or heading, which Markdown's own grammar leaves to this one.
    Spec {
        name: "markdown_inline",
        aliases: &[],
        files: &[],
        grammar: tree_sitter_md::INLINE_LANGUAGE,
        highlights: &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        injections: &[query!("markdown_inline/injections.scm")],
        paints: &[],
    },
    Spec {
        name: "python",
        aliases: &["python3", "py3"],
        files: &[Extension("py"), Extension("pyi"), Extension("pyw")],
        grammar: tree_sitter_python::LANGUAGE,
        highlights: &[
            query!("python/highlights.scm"),
            // Helix's query paints an f-string's `{x}` as string. github.com shows the code in it plain.
            "(interpolation) @none",
        ],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "rst",
        aliases: &["restructuredtext", "rest"],
        files: &[Extension("rst")],
        grammar: tree_sitter_rst::LANGUAGE,
        highlights: &[query!("rst/highlights.scm")],
        injections: &[query!("rst/injections.scm")],
        paints: &[],
    },
    Spec {
        name: "rust",
        aliases: &[],
        files: &[Extension("rs")],
        grammar: tree_sitter_rust::LANGUAGE,
        highlights: &[
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            // The query's own pattern for these has a stray `'` in its regex, and matches nothing.
            r#"((identifier) @constant (#match? @constant "^[A-Z][A-Z\\d_]+$"))"#,
        ],
        injections: &[tree_sitter_rust::INJECTIONS_QUERY],
        paints: &[],
    },
    // Svelte's grammar is HTML's with Svelte's blocks and braces added, and its query adds to HTML's.
    Spec {
        name: "svelte",
        aliases: &[],
        files: &[Extension("svelte")],
        grammar: tree_sitter_svelte_ng::LANGUAGE,
        highlights: &[
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_svelte_ng::HIGHLIGHTS_QUERY,
            QUOTED_ATTRIBUTE,
        ],
        injections: &[query!("svelte/injections.scm")],
        paints: &[],
    },
    Spec {
        name: "toml",
        aliases: &[],
        files: &[Extension("toml"), Name("Cargo.lock")],
        grammar: tree_sitter_toml_ng::LANGUAGE,
        highlights: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        injections: &[],
        // The query paints a key as a type, and the whole of a key's line as a property.
        paints: &[("type", Some(Highlight::Tag)), ("property", None)],
    },
    // TypeScript's query adds to JavaScript's.
    Spec {
        name: "typescript",
        aliases: &[],
        files: &[Extension("ts"), Extension("mts"), Extension("cts")],
        grammar: tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        highlights: &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            // A name in capitals, which JavaScript's query paints as a constant, and TypeScript's as a type.
            r#"((identifier) @constant (#match? @constant "^[A-Z_][A-Z\\d_]+$"))"#,
        ],
        injections: &[],
        paints: &[],
    },
    Spec {
        name: "yaml",
        aliases: &[],
        files: &[Extension("yml"), Extension("yaml")],
        grammar: tree_sitter_yaml::LANGUAGE,
        highlights: &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
        injections: &[],
        paints: &[("property", Some(Highlight::Tag))],
    },
];

/// An HTML attribute's value with its quotes, which github.com shows as a string. HTML's query paints the
/// text inside them.
const QUOTED_ATTRIBUTE: &str = "(quoted_attribute_value) @string";
