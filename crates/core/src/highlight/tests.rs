use std::path::Path;

use super::*;

/// The language Tsugiki calls `name`.
fn language(name: &str) -> Language {
    Language::named(name).unwrap()
}

/// Lines `lines` of `source`, each span drawn as `[highlight text]`.
fn drawn(language: Language, source: &str, lines: Range<u32>) -> Vec<String> {
    let parsed = Parsed::parse(language, source.to_owned()).unwrap();
    let texts: Vec<&str> = source.split('\n').collect();
    let spans = parsed.spans(lines.clone());
    lines
        .zip(spans)
        .map(|(line, spans)| {
            let text = texts.get(usize::try_from(line).unwrap() - 1).unwrap_or(&"");
            with_spans(text, &spans)
        })
        .collect()
}

/// Every line of `source`, drawn as `drawn` draws them.
fn drawn_whole(language: Language, source: &str) -> Vec<String> {
    let lines = u32::try_from(source.lines().count()).unwrap();
    drawn(language, source, 1..lines + 1)
}

/// `text` with each span drawn as `[highlight text]`.
pub(crate) fn with_spans(text: &str, spans: &[Span]) -> String {
    let text: Vec<u16> = text.encode_utf16().collect();
    let piece = |from: u32, to: u32| {
        let range = usize::try_from(from).unwrap()..usize::try_from(to).unwrap();
        String::from_utf16(text.get(range).unwrap()).unwrap()
    };
    let mut out = String::new();
    let mut at = 0;
    for span in spans {
        out.push_str(&piece(at, span.start));
        out.push_str(&format!(
            "[{:?} {}]",
            span.highlight,
            piece(span.start, span.end)
        ));
        at = span.end;
    }
    out + &piece(at, u32::try_from(text.len()).unwrap())
}

#[test]
fn a_file_is_highlighted_by_its_extension_or_its_name() {
    let languages = [
        "Objects/typeobject.c",
        "Include/object.h",
        "Lib/typing.pyi",
        "Doc/whatsnew/3.15.rst",
        "Makefile.pre.in",
        "ui/src/App.svelte",
        "Lib/dir.py/README",
        "Lib/.py",
    ]
    .map(|path| Language::of(path).map(|language| language.spec.name));

    assert_eq!(
        languages,
        [
            Some("c"),
            Some("c"),
            Some("python"),
            Some("rst"),
            Some("make"),
            Some("svelte"),
            None,
            None
        ]
    );
}

#[test]
fn a_fence_may_name_a_language_by_its_name_an_alias_or_an_extension() {
    let named = [
        "python",
        " Python3 ",
        "py",
        "ts",
        "rs",
        "markdown_inline",
        "pycon",
    ]
    .map(|name| Language::named(name).map(|language| language.spec.name));

    assert_eq!(
        named,
        [
            Some("python"),
            Some("python"),
            Some("python"),
            Some("typescript"),
            Some("rust"),
            Some("markdown_inline"),
            None
        ]
    );
}

#[test]
fn each_language_s_queries_compile_and_use_only_the_predicates_tsugiki_reads() {
    for (index, spec) in LANGUAGES.iter().enumerate() {
        let name = spec.name;
        let grammar = Language { spec, index }.grammar();
        let grammar = grammar.unwrap_or_else(|| panic!("{name}'s queries compile"));
        let injections = grammar
            .injections
            .iter()
            .map(|injections| &injections.query);
        for query in std::iter::once(&grammar.highlights).chain(injections) {
            for pattern in 0..query.pattern_count() {
                // tree-sitter's Rust binding checks `#eq?`, `#match?` and `#any-of?`, and reads `#set!`.
                // It leaves the rest, such as Neovim's `#lua-match?`, to its caller, which Tsugiki isn't.
                let predicates = query.general_predicates(pattern);
                assert!(
                    predicates.is_empty(),
                    "{name}'s pattern {pattern} has {predicates:?}"
                );
            }
        }
    }
}

#[test]
fn no_two_languages_claim_the_same_file_or_name() {
    let mut files = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for spec in LANGUAGES {
        for file in spec.files {
            assert!(files.insert(*file), "two languages claim {file:?}");
        }
        let extensions = spec.files.iter().filter_map(|file| match *file {
            File::Extension(extension) => Some(extension),
            File::Name(_) => None,
        });
        let aliases = spec.aliases.iter().copied();
        let mut called: Vec<&str> = [spec.name]
            .into_iter()
            .chain(aliases)
            .chain(extensions)
            .collect();
        called.sort_unstable();
        called.dedup();
        for name in called {
            assert!(names.insert(name), "two languages are called {name}");
            assert_eq!(name, name.to_lowercase(), "{name} is in lower case");
        }
    }
}

#[test]
fn each_sample_is_highlighted_as_its_snapshot_shows() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/highlight/samples");
    let mut samples: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    samples.sort();
    let mut highlighted = Vec::new();
    for sample in samples {
        let name = sample.file_name().unwrap().to_str().unwrap().to_owned();
        let language = Language::of(&name).unwrap_or_else(|| panic!("Tsugiki highlights {name}"));
        let source = std::fs::read_to_string(&sample).unwrap();
        let drawn = drawn_whole(language, &source).join("\n");
        assert!(drawn.contains('['), "{name} has highlights");
        insta::assert_snapshot!(name, drawn);
        highlighted.push(language);
    }
    for (index, spec) in LANGUAGES.iter().enumerate() {
        let name = spec.name;
        let sampled = highlighted.contains(&Language { spec, index });
        assert!(sampled || spec.files.is_empty(), "{name} has a sample");
    }
}

#[test]
fn a_query_that_doesn_t_compile_or_has_no_content_leaves_its_language_plain() {
    let broken = Spec {
        name: "broken",
        aliases: &[],
        files: &[],
        grammar: tree_sitter_json::LANGUAGE,
        highlights: &["(no_such_node) @string"],
        injections: &[],
        paints: &[],
    };
    let no_content = Spec {
        highlights: &[tree_sitter_json::HIGHLIGHTS_QUERY],
        injections: &["(string) @injection.language"],
        ..broken
    };

    assert!(Grammar::compile(&broken).is_none());
    assert!(Grammar::compile(&no_content).is_none());
}

#[test]
fn c_shows_the_preprocessor_types_strings_and_calls_and_a_macro_s_body_as_c() {
    let source = "\
#include <Python.h>
#define LIMIT 10

static int
check(PyObject *op)
{
    /* A comment. */
    return PyErr_Format(PyExc_ValueError, \"bad %d\", LIMIT) == NULL;
}
";

    assert_eq!(
        drawn_whole(language("c"), source),
        [
            "[Keyword #include] [String <Python.h>]",
            "[Keyword #define] [Constant LIMIT] [Constant 10]",
            "",
            "[Keyword static] [Type int]",
            // Helix's query takes the `*` of a pointer for part of its type.
            "[Function check]([Type PyObject] [Type *]op)",
            "{",
            "    [Comment /* A comment. */]",
            // Helix's query leaves a call's arguments plain, after the pattern that paints a constant.
            "    [Keyword return] [Function PyErr_Format](PyExc_ValueError, [String \"bad %d\"], LIMIT) \
             [Keyword ==] [Constant NULL];",
            "}",
        ]
    );
}

#[test]
fn a_chunk_reads_only_its_lines_and_a_node_from_above_paints_from_its_first_line() {
    let source = "x = 1\ndoc = \"\"\"one\ntwo\nthree\"\"\"\ny = 2\n";

    assert_eq!(
        drawn(language("python"), source, 3..6),
        [
            "[String two]",
            "[String three\"\"\"]",
            "y [Keyword =] [Constant 2]"
        ]
    );
}

#[test]
fn of_the_patterns_that_capture_one_node_the_last_in_the_query_wins() {
    // Helix's Python query captures a capitalised name as a type, and one in capitals after that as a
    // constant. A call is a constructor's after the first.
    assert_eq!(
        drawn(language("python"), "LIMIT = Point(1)\n", 1..2),
        ["[Constant LIMIT] [Keyword =] [Type Point]([Constant 1])"]
    );
}

#[test]
fn a_markdown_fence_shows_its_language_and_one_tsugiki_doesn_t_know_shows_plain() {
    let source = "\
# The *first* heading, with a <kbd>key</kbd>

```python
def f(): return 1
```

> ```c
> int x = 1; /* one
> two */
> ```

```pycon
>>> f()
```
";

    assert_eq!(
        drawn_whole(language("markdown"), source),
        [
            // A paragraph's HTML tags parse as one, so a closing tag shows as its opening one does.
            "# [Heading The ][Emphasis *first*][Heading  heading, with a <][Tag kbd][Heading >key</][Tag kbd]\
             [Heading >]",
            "",
            "[Constant ```python]",
            "[Keyword def] [Function f](): [Keyword return] [Constant 1]",
            "[Constant ```]",
            "",
            "> [Constant ```c]",
            "> [Type int] x [Keyword =] [Constant 1]; [Comment /* one]",
            // The comment runs over the quote's `>`, which the C wasn't parsed from.
            "> [Comment two */]",
            "> [Constant ```]",
            "",
            "[Constant ```pycon]",
            ">>> f()",
            "[Constant ```]",
        ]
    );
}

#[test]
fn svelte_shows_a_script_and_a_style_in_their_languages() {
    let svelte = "\
<script lang=\"ts\">let x: number = 1;</script>
<style lang=\"scss\">p { color: red; }</style>
<p>{x + 1}</p>
";

    assert_eq!(
        drawn_whole(language("svelte"), svelte),
        [
            "<[Tag script] [Function lang]=[String \"ts\"]>[Keyword let] x: [Type number] [Keyword =] \
             [Constant 1];</[Tag script]>",
            "<[Tag style] [Function lang]=[String \"scss\"]>[Tag p] { [Constant color]: red; }</[Tag style]>",
            "<[Tag p]>{x [Keyword +] [Constant 1]}</[Tag p]>",
        ]
    );
}

#[test]
fn code_is_parsed_at_most_four_layers_in() {
    let source = "a!(b!(c!(d!(e!(f!(1))))));\n";

    let parsed = Parsed::parse(language("rust"), source.to_owned()).unwrap();

    assert!(format!("{parsed:?}").starts_with("Parsed { language: rust, lines: 2, layers: 5"));
}

#[test]
fn spans_count_utf16_code_units_as_the_webview_does() {
    let parsed = Parsed::parse(language("python"), "s = \"ü😀\" # x\n".to_owned()).unwrap();

    let spans = parsed.spans(1..2);

    let at = |start, end, highlight| Span {
        start,
        end,
        highlight,
    };
    assert_eq!(
        spans,
        [vec![
            at(2, 3, Highlight::Keyword),
            at(4, 9, Highlight::String),
            at(10, 13, Highlight::Comment)
        ]]
    );
}

#[test]
fn lines_past_the_end_have_no_spans() {
    let parsed = Parsed::parse(language("c"), "int x;".to_owned()).unwrap();

    assert_eq!(
        parsed.spans(1..4).iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 0, 0]
    );
    assert_eq!(parsed.spans(5..5), Vec::<Vec<Span>>::new());
}

#[test]
fn the_longest_part_of_a_capture_s_name_with_a_rule_decides_what_it_paints() {
    let paint = |capture| Paint::of(capture, &[]);
    let yaml = |capture| {
        Paint::of(
            capture,
            &[("property", Some(Highlight::Tag)), ("label", None)],
        )
    };

    assert_eq!(
        paint("function.builtin"),
        Some(Paint::Color(Highlight::Function))
    );
    assert_eq!(
        paint("constant.character"),
        Some(Paint::Color(Highlight::String))
    );
    assert_eq!(
        paint("constant.character.escape"),
        Some(Paint::Color(Highlight::Constant))
    );
    assert_eq!(paint("tag.delimiter"), None);
    assert_eq!(paint("embedded"), Some(Paint::Plain));
    assert_eq!(paint("variable.parameter"), None);
    assert_eq!(paint("_ann"), None);
    assert_eq!(yaml("property"), Some(Paint::Color(Highlight::Tag)));
    assert_eq!(yaml("label.anchor"), None);
}

#[test]
fn a_file_s_extension_is_what_follows_the_last_dot_of_a_name_with_more_before_it() {
    let rs = File::Extension("rs");

    assert!(rs.holds("lib.rs"));
    assert!(!rs.holds(".rs"));
    assert!(!rs.holds("libers"));
    assert!(File::Name("Makefile").holds("Makefile"));
}

/// 13,000 lines of C, as the `highlight` bench has, whose tree takes 7.8 MB.
fn long_c() -> String {
    let function = |n: u32| {
        [
            format!("/* Checks the object's slot {n}, and says why it can't. */"),
            "static int".to_owned(),
            format!("check_slot_{n}(PyTypeObject *type, PyObject *value)"),
            "{".to_owned(),
            "    Py_ssize_t size = PyTuple_GET_SIZE(value);".to_owned(),
            format!("    if (size > {n} && type->tp_flags & Py_TPFLAGS_HEAPTYPE) {{"),
            "        PyErr_Format(PyExc_TypeError,".to_owned(),
            "                     \"%s.__slots__ has %zd items\",".to_owned(),
            "                     type->tp_name, size);".to_owned(),
            "        return -1;".to_owned(),
            "    }".to_owned(),
            "    for (Py_ssize_t i = 0; i < size; i++) {".to_owned(),
            "        PyObject *item = PyTuple_GET_ITEM(value, i);".to_owned(),
            "        if (!PyUnicode_Check(item)) {".to_owned(),
            "            return 0;".to_owned(),
            "        }".to_owned(),
            "    }".to_owned(),
            "    return 1;".to_owned(),
            "}".to_owned(),
            String::new(),
        ]
        .join(
            "
",
        )
    };
    (0..650).map(function).collect::<Vec<_>>().join(
        "
",
    )
}

#[test]
fn a_version_is_counted_at_no_less_than_its_tree_measured_and_not_twice_that() {
    let parsed = Parsed::parse(language("c"), long_c()).unwrap();

    let megabytes = parsed.cost() / 1_000_000;

    assert!((8..16).contains(&megabytes), "{megabytes} MB");
}

#[test]
fn a_parse_stops_when_asked_and_says_so() {
    let source = "Text\n\n```python\nx = 1\n```\n";
    let mut checks = 0;
    let whole = Parsed::parse_unless(language("markdown"), source.to_owned(), &mut || {
        checks += 1;
        false
    });
    assert!(matches!(whole, Ok(Some(_))));

    // The last check comes after the file's own tree, before a layer inside it.
    let mut left = checks;
    let stopped = Parsed::parse_unless(language("markdown"), source.to_owned(), &mut || {
        left -= 1;
        left == 0
    });
    assert!(matches!(stopped, Err(Stopped)));
    let at_once = Parsed::parse_unless(language("c"), long_c(), &mut || true);
    assert!(matches!(at_once, Err(Stopped)));
}
