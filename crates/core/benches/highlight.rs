//! Highlighting a C file about as long as CPython's `Objects/typeobject.c` (ADR 0001, ADR 0017). A chunk
//! should take about a millisecond, and a parse well under the 100 ms a jump into a file has.

use tsugiki_core::highlight::{Language, Parsed};

fn main() {
    divan::main();
}

/// Functions in the file, of 20 lines each: 13,000 lines, as `typeobject.c` has.
const FUNCTIONS: u32 = 650;

/// The rows the webview asks for at a time, as `CHUNK` in `pr-files.svelte.ts`.
const CHUNK: u32 = 512;

/// Parsing the whole file, which a jump into a file Tsugiki hasn't parsed waits for.
#[divan::bench]
fn parse(bencher: divan::Bencher<'_, '_>) {
    let source = c_file();
    bencher.bench(|| Parsed::parse(Language::C, source.clone()));
}

/// The spans of a chunk from the middle of the file, once it's parsed.
#[expect(
    clippy::expect_used,
    reason = "a benchmark that timed a file that didn't parse would measure nothing the user waits for"
)]
#[divan::bench]
fn chunk(bencher: divan::Bencher<'_, '_>) {
    let parsed = Parsed::parse(Language::C, c_file()).expect("the C parses");
    let start = FUNCTIONS * 10;
    let spans = parsed.spans(start..start + CHUNK);
    assert!(spans.iter().filter(|line| !line.is_empty()).count() > 300);
    bencher.bench(|| parsed.spans(start..start + CHUNK));
}

/// C in CPython's style: a comment, a function with a declaration, a call with a string, a branch and a
/// macro, 20 lines each.
fn c_file() -> String {
    let mut lines = vec!["#include \"Python.h\"".to_owned(), String::new()];
    for function in 0..FUNCTIONS {
        lines.extend([
            format!("/* Checks the object's slot {function}, and says why it can't. */"),
            "static int".to_owned(),
            format!("check_slot_{function}(PyTypeObject *type, PyObject *value)"),
            "{".to_owned(),
            "    Py_ssize_t size = PyTuple_GET_SIZE(value);".to_owned(),
            format!("    if (size > {function} && type->tp_flags & Py_TPFLAGS_HEAPTYPE) {{"),
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
        ]);
    }
    lines.join("\n")
}
