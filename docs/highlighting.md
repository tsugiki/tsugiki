# Highlighting

The Files tab colors code the way github.com does, with [tree-sitter](https://tree-sitter.github.io). This
page says how, and how to add a language. You don't need to know the rest of Tsugiki to add one: it touches a
table, a sample file and that sample's snapshot.

## How it works

Tsugiki parses each version of a file whole, once. For each chunk of rows the diff shows, it runs the
language's highlights query over those rows' lines only, so a chunk costs about a millisecond whatever the
file's length ([ADR 0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)).

A highlights query names nodes with captures such as `@keyword` or `@function.builtin`. `Paint::shared` in
`crates/core/src/highlight.rs` maps each capture to one of a few highlights, by the longest leading part of
its name that has a rule: `@function.builtin` paints as `@function` does. It leaves the rest, such as
`@variable` and `@punctuation`, plain. `DiffRows.svelte` paints each highlight with a token of the **Theme**
in force, from its `[syntax]` table ([devel/themes.md](devel/themes.md)).

| Highlight | Painted with | Captures, among others |
|---|---|---|
| keyword | `keyword` | `@keyword`, `@operator` |
| string | `string` | `@string`, `@markup.link` |
| comment | `comment` | `@comment` |
| constant | `constant` | `@constant`, `@number`, `@string.escape`, `@markup.raw` |
| function, type | `entity` | `@function`, `@attribute`, `@type`, `@constructor` |
| tag | `tag` | `@tag` |
| heading | `constant`, bold | `@markup.heading`, `@text.title` |
| strong, emphasis | bold, italic | `@markup.bold`, `@markup.italic` |

Of two patterns that capture one node, the later in the query wins, as in Helix and Neovim, even when its
capture paints nothing: Helix's C query leaves a call's arguments plain after it paints constants. A node
inside another paints over it.

Code in one language inside another, such as a Markdown code fence, an HTML `<script>` or a reST
`code-block`, comes from the language's injections query. Tsugiki parses each piece as a layer of its own,
whose captures paint over the file's. It reads the parts of an injections query that tree-sitter's own
highlighter does:

- `@injection.content` is the code.
- `@injection.language` is a node whose text names its language, or `#set! injection.language "css"` names
  it in the pattern.
- `#set! injection.include-children` keeps the nodes inside the code. Without it, Tsugiki leaves them out,
  such as the `>` of a quote around a Markdown fence.
- `#set! injection.combined` parses all the pieces a pattern finds in a layer as one, so the HTML tags of a
  Markdown paragraph open and close each other.

A name finds a language by its `name`, one of its `aliases`, or one of the extensions in its `files`, so
`py` finds Python. Code in a language Tsugiki doesn't highlight shows as the text around it does. Tsugiki
stops four layers in.

tree-sitter checks `#eq?`, `#not-eq?`, `#match?`, `#any-of?` and their `#any-` forms, and Tsugiki reads
`#set!`. A query that uses anything else, such as Neovim's `#lua-match?`, `#has-ancestor?` or `#offset!`,
fails a test, since Tsugiki would ignore it and highlight what the pattern meant to leave out.

## Add a language

1. Find the grammar's crate on crates.io. It must depend on `tree-sitter-language`, as the crates in
   `Cargo.toml` do, and its licence must be one `deny.toml` allows: `just deny` checks.
2. Add the crate to `[workspace.dependencies]` in `Cargo.toml`, and as `tree-sitter-x.workspace = true` to
   `crates/core/Cargo.toml`.
3. Add an entry to `LANGUAGES` in `crates/core/src/highlight/languages.rs`, in order of name. The doc comments
   on `Spec` in `crates/core/src/highlight.rs` say what each field holds.
4. Add a sample to `crates/core/src/highlight/samples`, named so that `files` picks it: `yaml.yaml`,
   `Makefile`. About 15 lines that show keywords, strings, comments, numbers, functions, types and any code
   in another language are enough. Use placeholder names, not a real person's.
5. Run `just highlight-samples`. It writes the sample's snapshot to `crates/core/src/highlight/snapshots`,
   where each run of color shows as `[Keyword def]`. Read it as you would github.com's view of the file.
6. Fix what doesn't look right, with the least that does it:
   - A capture that paints the wrong thing in this language: an entry in `paints`, such as YAML's
     `("property", Some(Highlight::Tag))` for a key.
   - A pattern the query lacks, or one that loses to a pattern after it: a pattern of your own after the
     crate's query in `highlights`, with a comment that says why. JSON's entry has one.
   - A crate with no query, or one written for Neovim: a query copied from Helix, as the next section says.
7. Run `cargo test -p tsugiki-core --lib highlight`. Its tests check that each query compiles and uses only
   the predicates above, that no two languages claim the same file or name, and that each language has a
   sample.
8. In the PR, say how much the grammar adds to the app: build `just build` with and without it, and compare
   the size of `target/release/tsg.exe`.

## Queries from elsewhere

Use the query the grammar's crate ships. If it has none, or one written for Neovim, copy Helix's from
[`runtime/queries`](https://github.com/helix-editor/helix/tree/master/runtime/queries). Helix writes its
queries for a later pattern to win and uses only the predicates above. Neovim's use predicates Tsugiki lacks.

Put the copy in `crates/core/queries/<name>/` and start it with the header the others have:

- the link to the file at the commit you copied it from;
- what you changed, or "Unchanged";
- the Mozilla Public License's notice, which its Exhibit A gives. Helix's queries are under MPL-2.0, whose
  text is in `crates/core/queries/LICENSES`.

A Helix query that starts with `; inherits: html` adds to HTML's: list HTML's query first in `highlights`.

To update a copy, fetch the file at a newer commit, put the header back with the new link, and run
`just highlight-samples` to see what changed.

A query Tsugiki writes itself says "Tsugiki's own" at the top.
