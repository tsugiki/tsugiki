# Which Rust highlighter fits the diff

Research for [#153](https://github.com/tsugiki/tsugiki/issues/153). Context: [ADR
0001](../docs/adr/0001-standalone-tauri-gui-with-in-app-review.md) (highlighting runs in Rust, rows stay in
Rust, the webview asks for row ranges in chunks, and a file is highlighted when it is first asked for) and
[ADR 0017](../docs/adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)
(jumping into a file not yet highlighted: 100 ms).

## Answer

Use tree-sitter: the `tree-sitter` crate and one grammar crate per language. Don't use syntect. Parse each
file whole, once, and keep its `Tree`. Answer each chunk request by running the language's highlights query
over that chunk's byte range only (`QueryCursor::set_byte_range`). A cold jump into the middle of
`Objects/typeobject.c` then costs 48 ms to parse plus 1.4 ms for the chunk. `Modules/posixmodule.c`, the
largest C file under `Objects/`, `Modules/` and `Python/`, costs 67 + 1 ms. Both fit the 100 ms budget, with
time left for IPC.

syntect can't fit that budget. Its state goes line by line from the top of the file, so the first chunk at
row N costs parsing every row above N. A jump to the end of `typeobject.c` pays 277 ms for its 13,200 rows
before any theme is applied. With the theme, syntect runs at about 33k lines/s with Oniguruma, which matches
the prototype's 35k rows/s. Saving its state every 512 lines makes later
chunks cheap (7 to 23 ms each), but only once the whole file has gone through.

Don't call `tree-sitter-highlight` for chunks either. Its `highlight()` takes the whole source, parses it
again on every call and has no range parameter. Tsugiki writes a small highlighter of its own over
`QueryCursor`, which has to handle overlapping captures and injections (Markdown inline, `<script>` and
`<style>` in HTML, code blocks in reST). Helix's `tree-house` already does range highlighting across
injections, so it is worth reading before writing ours.

tree-sitter's costs, against syntect's:

- **The queries are ours to keep.** `tree-sitter-rst` ships no query files at all, and the upstream C and
  Python queries are short. Expect to vendor highlight queries, from Helix (MPL-2.0) or nvim-treesitter
  (Apache-2.0), and to keep a notice with them.
- **The binary grows by 5.3 MB** for the eleven grammars, against 1.6 MB for syntect with two-face.
- **The clean release build takes 30 s** with 4 jobs, against 25 s, and every grammar needs a C compiler.
  Tauri on Windows needs MSVC already.

## What the prototype used

The prototype on branch `prototype/diff-renderer` used `syntect = "5"` with default features, which select
the Oniguruma regex engine (`prototype/diff-renderer/src-tauri/Cargo.toml` on that branch). Its README says it
highlighted the diff lines only, "with one parse state for the old side and one for the new side, rather than
over the full files", and that "syntect manages about 35k rows/s". The worst cold jump, 214 ms on Windows,
came from highlighting a whole large file when the jump landed in it. ADR 0001 records the same numbers.

## Setup

- Windows 11, rustc 1.99.1, release profile with `lto = true`, `codegen-units = 1`, `strip = true`,
  `CARGO_BUILD_JOBS=4`, measured 2026-11-06 on a machine someone was using. Each number is the median of 5
  runs in one process unless it says "first".
- A scratch crate under `.temp/`, deleted afterwards. One binary per variant: tree-sitter 0.27.1 with
  tree-sitter-highlight 0.27.1 and the grammar crates below; syntect 5.3.0 with two-face 0.5.2 and
  `default-onig`; the same with `default-fancy`.
- Files from the CPython clone `X:\Work\cpython\main`, read only.
- A chunk is 512 rows, the prototype's chunk size, taken from the middle of the file.
- "syntect parse + highlight" runs `ParseState::parse_line` and `HighlightIterator` with the InspiredGitHub
  theme, as the prototype did. "Parse only" skips the theme, so it is the least a cold jump has to pay.
- For tree-sitter, "chunk" is a `QueryCursor` with `set_byte_range` over the highlights query on a tree that
  was already parsed. "Cold jump" is parse plus chunk. `Query::new` costs 0.2 to 6.8 ms once per language,
  and isn't counted.
- macOS was not measured. The prototype found its Mac highlighted about twice as fast.

## Speed (measured)

Times in ms.

| File | Lines | syntect whole file | syntect parse only | syntect chunk from a saved state | tree-sitter parse | tree-sitter-highlight whole file | tree-sitter chunk | tree-sitter cold jump |
|---|---|---|---|---|---|---|---|---|
| `Objects/typeobject.c` | 13,200 | 399 | 277 | 15.7 | 48.0 | 88.2 | 1.4 | 54.5 |
| `Modules/posixmodule.c` | 19,317 | 465 | 331 | 11.4 | 66.5 | 104 | 0.9 | 68.8 |
| `Include/object.h` | 805 | 18.9 | 14.9 | 13.0 | 2.1 | 3.5 | 1.0 | 3.3 |
| `Lib/_pydecimal.py` | 6,530 | 204 | 177 | 21.9 | 21.3 | 38.8 | 1.5 | 26.7 |
| `Lib/typing.py` | 3,956 | 141 | 106 | 22.8 | 14.0 | 23.6 | 1.2 | 15.6 |
| `Doc/whatsnew/3.12.rst` | 2,282 | 33.9 | 24.5 | 6.8 | 26.2 | 42.0 | 2.3 | 36.4 |
| `Makefile.pre.in` | 3,564 | 93.1 | 64.8 | 11.0 | 7.1 | 14.9 | 1.2 | 8.5 |
| `InternalDocs/parser.md` | 987 | 19.1 | 20.0 | 8.2 | 10.1 | 20.3 | 1.3 | 13.9 |
| `Misc/stable_abi.toml` | 2,918 | 32.5 | 21.1 | 6.6 | 7.9 | 14.3 | 1.5 | 9.1 |
| `.github/workflows/build.yml` | 683 | 18.0 | 19.1 | 14.2 | 2.6 | 3.6 | 1.0 | 3.0 |
| `configure` (Generated) | 40,736 | 1,235 | 969 | 15.4 | 159 | 268 | 1.0 | 189 |

- **tree-sitter parses 5 to 9 times faster than syntect** on C, Python, Makefiles and YAML, and 2 to 3 times
  on Markdown and TOML. On reST the two are close. tree-sitter-rst and tree-sitter-md parse at 87k and 98k
  lines/s, and the other grammars at 260k to 500k.
- **The cold jump is dominated by the parse.** The chunk query costs 1 to 2 ms whatever the file's size.
- **The first run is slower.** The first tree-sitter cold jump into `typeobject.c` took 82.8 ms, with a 48.5
  ms parse, so the spread on this machine was wide. The first syntect run of a syntax also compiles its
  regexes.
- **fancy-regex is out.** With `default-fancy`, syntect took 919 ms for `typeobject.c`, 486 ms for
  `_pydecimal.py` and 2,792 ms for `configure`, 2 to 3 times slower than Oniguruma. On `3.12.rst` it took 884
  ms, 26 times slower, and 244 ms for one chunk. syntect's README says fancy-regex "seems to be about half the
  speed of the default Oniguruma engine" ([syntect README](https://github.com/trishume/syntect)).
- **`configure` is over budget for both.** It is marked `generated` in CPython's `.gitattributes`, so it
  starts collapsed. Parse it when it is expanded, or in the background, and show its rows plain until the
  parse is done.

## Highlighting one chunk without the whole file before it

**syntect.** `ParseState` and `HighlightState` carry the state from one line into the next, and there is no
safe place to restart: inside a C block comment or a Python docstring, a state started fresh is wrong. The
README's advice for editors is to "copy the parse state into a side-buffer" every 1000 lines or so on the
first parse ([syntect README](https://github.com/trishume/syntect)). That makes later chunks cheap, but the
first request for row N still pays for parsing every row above N. The prototype's simplification, one state
per side over the diff lines only, is the wrong-state case, and its README says so.

**tree-sitter.** tree-sitter can't resume a parse either. Parsing a whole file fits the budget, and then a
chunk needs no state from the rows above it: `QueryCursor::set_byte_range` returns the captures of nodes
that overlap the range, so a string or comment that opens above the chunk still comes back. Other facts the
design depends on:

- `Tree` is `Send` and `Sync`, and `Tree::clone` calls `ts_tree_copy`, documented as "a shallow copy of the
  syntax tree. This is very fast" (`binding_rust/lib.rs` and `include/tree_sitter/api.h` in tree-sitter
  0.27.1). One thread can parse and hand copies of each tree to whoever answers chunk requests. That fits the
  "one writer, many readers" rule in `AGENTS.md`.
- `Parser::set_included_ranges` parses only the given ranges of a document ([docs.rs:
  Parser](https://docs.rs/tree-sitter/0.27.1/tree_sitter/struct.Parser.html)). Parsing only a diff's hunks
  would be cheaper, but it has syntect's wrong-state problem. Keep it as a fallback for files the size of
  `configure` at most.
- `parse_with_options` takes a progress callback, which can stop a parse that is no longer needed (same
  page).
- `tree-sitter-highlight`'s `Highlighter::highlight` takes `source: &[u8]` and an optional cancellation flag,
  with no tree and no range ([docs.rs:
  Highlighter](https://docs.rs/tree-sitter-highlight/0.27.1/tree_sitter_highlight/struct.Highlighter.html)).
  It is useful in tests, as the whole-file answer a chunk highlighter must agree with.
- `tree-house`'s `Highlighter::new(syntax, src: RopeSlice, loader, range)` highlights a range of a parsed
  `Syntax`, across injections ([docs.rs:
  tree-house](https://docs.rs/tree-house/0.4.0/tree_house/highlighter/struct.Highlighter.html)). It needs a
  `ropey` rope, its README says to expect breaking changes and unfinished docs, and it is MPL-2.0
  ([helix-editor/tree-house](https://github.com/helix-editor/tree-house)).

Both libraries need each file's full text. Tsugiki would read the old and new blobs from the mirror and map
diff rows to their lines. I didn't measure the cost of that read.

## Languages a CPython PR touches

| Language | syntect with two-face (onig) | tree-sitter crate | Notes on tree-sitter |
|---|---|---|---|
| Python | Python | `tree-sitter-python` 0.25.0 (2025-09) | `typing.py` has one ERROR node: `lazy` imports (PEP 810) on main are newer than the grammar. |
| C and headers | C | `tree-sitter-c` 0.24.2 (2026-04) | Macros such as `PyVarObject_HEAD_INIT(...)` give local ERROR nodes: 74 covering 2.7 KB of `typeobject.c`'s 396 KB. |
| reST | reStructuredText | `tree-sitter-rst` 0.2.0 (2025-02) | Ships no queries: the `HIGHLIGHTS_QUERY` constants in its `lib.rs` are commented out. The bench used Helix's 38-line query. |
| Markdown | Markdown | `tree-sitter-md` 0.5.4 (2026-10) | Two grammars, block and inline, joined by an injection. |
| TOML | TOML, from two-face only | `tree-sitter-toml-ng` 0.7.0 (2024-12) | |
| YAML | YAML | `tree-sitter-yaml` 0.7.2 (2025-10) | |
| Shell | Bourne Again Shell (bash) | `tree-sitter-bash` 0.25.1 (2025-12) | |
| Makefiles | Makefile | `tree-sitter-make` 1.1.1 (2024-12, the only release) | 5 ERROR nodes in `Makefile.pre.in`. |
| HTML, CSS, JavaScript | HTML, CSS, JavaScript (Babel) | `tree-sitter-html` 0.23.2, `-css` 0.25.0, `-javascript` 0.25.0 | HTML injects JavaScript and CSS. |

- syntect's own syntax set has no TOML. two-face adds it, with the rest of bat's syntaxes ([two-face
  README](https://codeberg.org/CosmicHarper/two-face)). Under fancy-regex two-face drops some syntaxes,
  JavaScript (Babel) among them.
- All the tree-sitter grammar crates are on crates.io, MIT, and build against tree-sitter 0.27 through
  `tree-sitter-language`. `deny.toml` allows only crates.io sources.
- An ERROR node only affects the highlighting near it. syntect has no notion of an error: it highlights
  `lazy` as a name and moves on.

## License, binary size, build time, maintenance

| | tree-sitter + 11 grammars | syntect + two-face, onig | syntect + two-face, fancy-regex |
|---|---|---|---|
| Binary growth (stripped, LTO; empty binary 129 KB) | +5.3 MB (5,548 KB) | +1.6 MB (1,770 KB) | +2.5 MB (2,660 KB) |
| Clean release build, 4 jobs, one run | 30.4 s | 25.4 s | 37.2 s |
| Needs a C compiler | yes, core and each grammar | yes, Oniguruma (`onig_sys`) | no |
| `cargo deny check licenses` with Tsugiki's `deny.toml` | passes | passes | passes |
| `cargo deny check advisories` | clean | RUSTSEC-2025-0141 (bincode unmaintained), RUSTSEC-2024-0320 (yaml-rust unmaintained) | the same two |

- syntect's `parsing` feature turns on `dump-load`, which needs bincode (`syntect-5.3.0/Cargo.toml`), so
  syntect would need an advisory ignore in `deny.toml`. yaml-rust comes in through `yaml-load`, which a
  smaller feature list could leave out. syntect has open issues to replace bincode (#606, #623, #694).
- syntect bundles Sublime Text's syntaxes, whose license is a short permissive grant with no SPDX
  name ([sublimehq/Packages LICENSE](https://github.com/sublimehq/Packages)). cargo-deny only sees the
  crate's MIT.
- Vendored tree-sitter queries carry their own license: MPL-2.0 for Helix, Apache-2.0 for nvim-treesitter.
  `deny.toml` allows both, but cargo-deny won't check a file we copy in.

Maintenance, from crates.io and the GitHub API on 2026-11-06:

| Crate | Latest release | License | State |
|---|---|---|---|
| `tree-sitter`, `tree-sitter-highlight` | 0.27.1, 2026-10-08 | MIT | The reference implementation. Pushed to today, 27k stars. |
| `syntect` | 5.3.0, 2025-09-27 | MIT | "mostly complete, I still maintain it and review PRs, but it's not under heavy development" (README). Pushed 2026-10-31, and issue #621 plans a 6.0.0. |
| `two-face` | 0.5.2, 2026-08-07 | MIT OR Apache-2.0 | Follows bat's releases. |
| `tree-house` | 0.4.0, 2026-05-31 | MPL-2.0 | Helix's highlighter since 25.07. Breaking changes expected. |
| `arborium` | 2.18.2, 2026-08-28 | MIT OR Apache-2.0 | Bundles about 100 tree-sitter grammars with HTML rendering. Its feature list has no reST. |
| `lumis` (formerly `autumnus`) | 0.19.1, 2026-11-05 | MIT | Tree-sitter with Neovim themes. Returns formatted HTML or ANSI, not spans. Requires `lumis-wasm-runtime`, MSRV 1.95. |
| `giallo` | 0.6.0, 2026-11-04 | EUPL-1.2 | Zola's highlighter, on VS Code's TextMate grammars. `deny.toml` doesn't allow EUPL-1.2. |
| `syntastica` | 0.6.1, 2025-06-19 | MPL-2.0 | No release in 17 months. |
| `inkjet` | 0.11.1, 2024-09-15 | MIT OR Apache-2.0 | Archived, on tree-sitter 0.23. |

None of the bundles (arborium, lumis, syntastica) highlights a byte range of a tree it already parsed, so
none of them solves the chunk problem better than the `tree-sitter` crate does.

## Not settled

- **How much memory a kept `Tree` takes** for a large file. Measure it before deciding how many to keep.
- **The query files.** Which ones to vendor, which capture names to map to CSS classes, and how they look
  next to github.com's highlighting.
- **The cost of reading the old and new blobs from the mirror** for each file of a PR.
- **macOS.**
- **Whether to adopt `tree-house`** or write our own range highlighter.
