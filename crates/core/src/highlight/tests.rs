use super::*;

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
fn a_file_is_highlighted_by_its_extension() {
    let languages = [
        "Objects/typeobject.c",
        "Include/object.h",
        "Lib/ast.py",
        "Lib/typing.pyi",
        "Doc/whatsnew/3.15.rst",
        "Makefile",
        "Lib/dir.py/README",
    ]
    .map(Language::of);

    assert_eq!(
        languages,
        [
            Some(Language::C),
            Some(Language::C),
            Some(Language::Python),
            Some(Language::Python),
            None,
            None,
            None
        ]
    );
}

#[test]
fn python_shows_keywords_strings_comments_numbers_functions_and_types() {
    let source = "\
import os

@cache
def walk(top: str, depth=2):
    # Goes down.
    print(f\"{top}/\", None)
    return os.path.join(top, \"x\\n\")
";

    assert_eq!(
        drawn(Language::Python, source, 1..8),
        [
            "[Keyword import] os",
            "",
            "[Function @cache]",
            "[Keyword def] [Function walk](top: [Type str], depth[Keyword =][Constant 2]):",
            "    [Comment # Goes down.]",
            "    [Function print]([String f\"]{top}[String /\"], [Constant None])",
            "    [Keyword return] os.path.[Function join](top, [String \"x][Constant \\n][String \"])",
        ]
    );
}

#[test]
fn c_shows_the_preprocessor_types_strings_and_calls() {
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
        drawn(Language::C, source, 1..10),
        [
            "[Keyword #include] [String <Python.h>]",
            // The C grammar leaves a macro's body as text, and its query takes only capitals for constants.
            "[Keyword #define] [Constant LIMIT] 10",
            "",
            "[Keyword static] [Type int]",
            "[Function check]([Type PyObject] [Keyword *]op)",
            "{",
            "    [Comment /* A comment. */]",
            "    [Keyword return] [Function PyErr_Format](PyExc_ValueError, [String \"bad %d\"], \
             [Constant LIMIT]) [Keyword ==] [Constant NULL];",
            "}",
        ]
    );
}

#[test]
fn a_chunk_reads_only_its_lines_and_a_node_from_above_paints_from_its_first_line() {
    let source = "x = 1\ndoc = \"\"\"one\ntwo\nthree\"\"\"\ny = 2\n";

    assert_eq!(
        drawn(Language::Python, source, 3..6),
        [
            "[String two]",
            "[String three\"\"\"]",
            "y [Keyword =] [Constant 2]"
        ]
    );
}

#[test]
fn of_the_patterns_that_capture_one_node_the_last_in_the_query_wins() {
    // Python's query captures a capitalised name as a constructor, and one in capitals after that as a
    // constant. A call is a function's after either.
    assert_eq!(
        drawn(Language::Python, "LIMIT = Point(1)\n", 1..2),
        ["[Constant LIMIT] [Keyword =] [Function Point]([Constant 1])"]
    );
}

#[test]
fn spans_count_utf16_code_units_as_the_webview_does() {
    let parsed = Parsed::parse(Language::Python, "s = \"ü😀\" # x\n".to_owned()).unwrap();

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
    let parsed = Parsed::parse(Language::C, "int x;".to_owned()).unwrap();

    assert_eq!(
        parsed.spans(1..4).iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 0, 0]
    );
    assert_eq!(parsed.spans(5..5), Vec::<Vec<Span>>::new());
    assert!(format!("{parsed:?}").starts_with("Parsed { language: C, lines: 1"));
}

#[test]
fn each_capture_name_in_both_queries_paints_as_the_github_look_has_it() {
    let painted = |grammar: &Grammar| -> Vec<(String, Option<Paint>)> {
        let names = grammar.query.capture_names().iter();
        names
            .map(|name| ((*name).to_owned(), Paint::of(name)))
            .collect()
    };

    let c = painted(&C);
    let python = painted(&PYTHON);

    assert!(c.contains(&("type".to_owned(), Some(Paint::Color(Highlight::Type)))));
    assert!(python.contains(&("embedded".to_owned(), Some(Paint::Plain))));
    assert!(python.contains(&("variable".to_owned(), None)));
    assert_eq!(
        Paint::of("attribute"),
        Some(Paint::Color(Highlight::Function))
    );
}
