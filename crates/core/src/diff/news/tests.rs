use super::*;
use crate::web::WebAddress;

const PATH: &str =
    "Misc/NEWS.d/next/Core_and_Builtins/2026-11-07-10-00-00.gh-issue-152631.LKgCM_.rst";

fn text(text: &str) -> NewsPiece {
    NewsPiece::Text {
        text: text.to_owned(),
    }
}

fn code(text: &str) -> NewsPiece {
    NewsPiece::Code {
        text: text.to_owned(),
    }
}

fn link(text: &str, to: LinkTo) -> NewsPiece {
    NewsPiece::Link {
        text: text.to_owned(),
        to,
    }
}

fn web(url: &str) -> LinkTo {
    LinkTo::Web {
        url: WebAddress::parse(url).unwrap(),
    }
}

/// The pieces of `source`'s only paragraph.
fn pieces(source: &str) -> Vec<NewsPiece> {
    let entry = NewsEntry::render(PATH, source);
    match entry.blocks.as_slice() {
        [NewsBlock::Paragraph { pieces }] => pieces.clone(),
        blocks => panic!("not one paragraph: {blocks:?}"),
    }
}

#[test]
fn an_entry_is_a_rst_file_under_next() {
    assert!(NewsEntry::is_entry(PATH));
    assert!(!NewsEntry::is_entry("Misc/NEWS.d/3.14.0a1.rst"));
    assert!(!NewsEntry::is_entry("Misc/NEWS.d/next/Library/README.txt"));
    assert!(!NewsEntry::is_entry("Lib/next/Library/x.rst"));
}

#[test]
fn an_entry_names_its_section_and_issue_from_its_path() {
    let entry = NewsEntry::render(PATH, "Fixed.\n");

    assert_eq!(entry.path, PATH);
    assert_eq!(entry.section, "Core and Builtins");
    assert_eq!(
        entry.issue,
        Some(NewsLink {
            text: "gh-152631".to_owned(),
            to: LinkTo::GhIssue { number: 152_631 },
        })
    );
    let old = "Misc/NEWS.d/next/Tools-Demos/2021-05-24-14-41-29.bpo-44170._v28mv.rst";
    let old = NewsEntry::render(old, "Fixed.\n");
    assert_eq!(old.section, "Tools/Demos");
    assert_eq!(
        old.issue.map(|issue| (issue.text, issue.to)),
        Some(("bpo-44170".to_owned(), LinkTo::Bpo { number: 44_170 }))
    );
    let odd = NewsEntry::render("Misc/NEWS.d/next/C_API/notes.rst", "Fixed.\n");
    assert_eq!((odd.section.as_str(), odd.issue), ("C API", None));
    for name in ["2026.gh-issue-x.rst", "2026.bpo-x.rst"] {
        let path = format!("Misc/NEWS.d/next/Library/{name}");
        assert_eq!(
            NewsEntry::render(
                &path, "Fixed.
"
            )
            .issue,
            None,
            "{name}"
        );
    }
}

#[test]
fn lines_of_a_paragraph_join_with_a_space() {
    assert_eq!(
        pieces("Fix a buffer overflow when\ninitializing from an\n  iterator.\n"),
        [text(
            "Fix a buffer overflow when initializing from an iterator."
        )]
    );
}

#[test]
fn roles_read_as_sphinx_shows_them() {
    let source = "Fix :func:`ast.parse`, :meth:`!open`, :class:`~collections.abc.Mapping`, \
                  :exc:`SyntaxError`, :c:func:`PyObject_Call` and :func:`print(x) <print>`.";

    assert_eq!(
        pieces(source),
        [
            text("Fix "),
            code("ast.parse()"),
            text(", "),
            code("open()"),
            text(", "),
            code("Mapping"),
            text(", "),
            code("SyntaxError"),
            text(", "),
            code("PyObject_Call()"),
            text(" and "),
            code("print(x)"),
            text("."),
        ]
    );
}

#[test]
fn roles_for_prose_read_as_prose_and_some_link_away() {
    let source = "See the :term:`GIL`, :ref:`the guide <guide>`, :program:`python`, :pep:`8`, :gh:`123`, \
                  :issue:`456`, :cve:`CVE-2026-1`, :rfc:`9110`, :pypi:`pip`, :pep:`the style <8>` and \
                  :pep:`next`.";

    assert_eq!(
        pieces(source),
        [
            text("See the "),
            text("GIL"),
            text(", "),
            text("the guide"),
            text(", "),
            NewsPiece::Strong {
                text: "python".to_owned()
            },
            text(", "),
            link("PEP 8", LinkTo::Pep { number: 8 }),
            text(", "),
            link("gh-123", LinkTo::GhIssue { number: 123 }),
            text(", "),
            link("bpo-456", LinkTo::Bpo { number: 456 }),
            text(", "),
            link(
                "CVE-2026-1",
                LinkTo::Cve {
                    id: "CVE-2026-1".to_owned()
                }
            ),
            text(", "),
            link("RFC 9110", LinkTo::Rfc { number: 9110 }),
            text(", "),
            link(
                "pip",
                LinkTo::Pypi {
                    project: "pip".to_owned()
                }
            ),
            text(", "),
            link("the style", LinkTo::Pep { number: 8 }),
            text(" and "),
            text("next"),
            text("."),
        ]
    );
}

#[test]
fn inline_markup_and_links_keep_their_kind() {
    let source = "Use *feature_version*, **not** ``SyntaxError``; see \
                  `the code <https://github.com/python/cpython>`_, https://github.com, `a target`_ and \
                  `bad <javascript:alert(1)>`_.";

    assert_eq!(
        pieces(source),
        [
            text("Use "),
            NewsPiece::Emphasis {
                text: "feature_version".to_owned()
            },
            text(", "),
            NewsPiece::Strong {
                text: "not".to_owned()
            },
            text(" "),
            code("SyntaxError"),
            text("; see "),
            link("the code", web("https://github.com/python/cpython")),
            text(", "),
            link("https://github.com", web("https://github.com")),
            text(", "),
            text("a target"),
            text(" and "),
            text("bad"),
            text("."),
        ]
    );
}

#[test]
fn a_reference_links_to_its_target_which_doesnt_show() {
    let source = "See `the Code`_ and `elsewhere`_.\n\n\
                  .. _the   code:\n   https://github.com/python/cpython\n";

    assert_eq!(
        pieces(source),
        [
            text("See "),
            link("the Code", web("https://github.com/python/cpython")),
            text(" and "),
            text("elsewhere"),
            text("."),
        ]
    );
}

#[test]
fn a_backslash_lets_markup_through_as_text() {
    assert_eq!(pieces(r"Match \*args."), [text("Match *args.")]);
}

#[test]
fn comments_go_lists_and_code_stay() {
    let source = ".. cve: CVE-2026-115557\n\nTwo changes:\n\n* First with ``code``.\n* Second\n  \
                  spanning lines.\n\nAn example::\n\n    x = 1\n      y = 2\n";

    let entry = NewsEntry::render(PATH, source);

    assert_eq!(
        entry.blocks,
        [
            NewsBlock::Paragraph {
                pieces: vec![text("Two changes:")]
            },
            NewsBlock::List {
                items: vec![
                    vec![NewsBlock::Paragraph {
                        pieces: vec![text("First with "), code("code"), text(".")]
                    }],
                    vec![NewsBlock::Paragraph {
                        pieces: vec![text("Second spanning lines.")]
                    }],
                ]
            },
            NewsBlock::Paragraph {
                pieces: vec![text("An example:")]
            },
            NewsBlock::Code {
                text: "x = 1\n  y = 2".to_owned()
            },
        ]
    );
}

#[test]
fn a_spaced_double_colon_goes_with_the_space() {
    let entry = NewsEntry::render(
        PATH,
        "For example ::

    x = 1
",
    );

    assert_eq!(
        entry.blocks,
        [
            NewsBlock::Paragraph {
                pieces: vec![text("For example")]
            },
            NewsBlock::Code {
                text: "x = 1".to_owned()
            },
        ]
    );
}

#[test]
fn a_lone_double_colon_leaves_only_the_code() {
    let entry = NewsEntry::render(PATH, "::\n\n    x = 1\n");

    assert_eq!(
        entry.blocks,
        [NewsBlock::Code {
            text: "x = 1".to_owned()
        }]
    );
}

#[test]
fn markup_tsugiki_doesnt_render_shows_as_its_text() {
    let entry = NewsEntry::render(PATH, ".. note::\n\n   Careful.\n");

    assert_eq!(
        entry.blocks,
        [NewsBlock::Paragraph {
            pieces: vec![text(".. note:: Careful.")]
        }]
    );
}

#[test]
fn backticks_without_a_role_read_as_a_title_and_a_role_can_follow() {
    assert_eq!(
        pieces("See `The Title` and `ast.parse`:func: here."),
        [
            text("See "),
            NewsPiece::Emphasis {
                text: "The Title".to_owned()
            },
            text(" and "),
            code("ast.parse()"),
            text(" here."),
        ]
    );
}

#[test]
fn inline_markup_tsugiki_doesnt_render_shows_as_its_text() {
    assert_eq!(
        pieces("Fixed [1]_ in |version|."),
        [
            text("Fixed "),
            text("[1]_"),
            text(" in "),
            text("|version|"),
            text("."),
        ]
    );
}
