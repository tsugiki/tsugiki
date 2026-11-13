# Go to definition asks a language server, which reads a Lookup checkout

Go to definition and the **Definition peek** ask a language server where a name is defined: ty for Python,
clangd for C and rust-analyzer for Rust. Tsugiki parses no language to answer that itself. The servers read a
**Lookup checkout**, a second worktree of the mirror that Tsugiki keeps at the base of whichever PR is on
screen, and they see the PR's changed files as documents Tsugiki holds open. Text search is a separate action,
with its own engine built from ripgrep's crates, and it reads the same checkout.

A name's definition follows the language's rules: `self` means the first parameter of the method it appears
in, a C `static` stays in its file, and an import decides which module a Python name comes from. A text search
can't apply those rules, and a parser that applies them for each language is a language server written again.
The servers already do that work. VS Code and Zed answer Go to definition with language servers too, and
both use clangd and rust-analyzer.

## How a lookup works

- **The Lookup checkout** is a detached worktree in `repos\<owner>__<name>\lookup\`, one per Repository. When
  a PR opens, Tsugiki moves it in the background to the PR's base (the merge-base), without asking. Neither a
  person nor an editor writes to it, and it is separate from the **Review checkout**, which the user opens in
  their editor and which moves only when they ask.
- **The base view is what's on disk. The head view adds the PR's changed files**, which Tsugiki opens on the
  server with their head text. A lookup on a removed line asks the base view, and every other lookup asks the
  head view. To switch views, Tsugiki replaces the open documents' text with `didChange`. The files stay open
  while the PR is on screen, because clangd goes on answering from a closed document's text.
- **The PR's added files are written into the Lookup checkout** while the PR is on screen, because ty finds
  imports on disk and not among open documents. The next move removes them.
- **A lookup never runs the PR's code.** rust-analyzer starts with build scripts and proc macros off. clangd
  reads compile flags that Tsugiki writes, and nothing is configured or built. ty runs no code.

## The servers

| Language | Server | Setup Tsugiki gives it |
|---|---|---|
| Python | ty | `environment.root = ["Lib"]` and the Python version, over LSP |
| C | clangd | A `compile_commands.json` with one entry for each core C file of the commit, all with the same flags, written to `repos\<owner>__<name>\servers\clangd\` beside the checkout, where clangd keeps its index too. Started with `--background-index --background-index-priority=low -j=4` |
| Rust | rust-analyzer | Build scripts and proc macros off |

- **The user supplies each server.** They unpack it themselves and pick its binary in Settings, and Tsugiki
  saves the path in `machine.toml`. Tsugiki downloads nothing. The docs and Settings name the versions we
  tested, and a different version gets a warning but is still used.
- **Tsugiki checks a server when the user sets its path**: that it starts, that `--version` names the right
  server and, for clangd, that its built-in headers sit next to the binary. While a server runs, Tsugiki
  watches what it reports: a missing system header, indexing that never starts, the process exiting. Each
  problem shows in the server's row in Settings with its log, and as a line in the peek in place of "No
  definition of X". A start runs no extra check: the version comes from the reply to `initialize`.
- **ty and clangd start when a PR that touches their language opens**, never when Tsugiki starts.
  rust-analyzer starts on the first lookup in Rust. Each stops after it has been idle for 30 minutes, which
  `machine.toml` can change.
- **The peek shows the server's answer, or "No definition of X".** While clangd builds its index, a definition
  in another file lands on its declaration in a header. The peek shows that answer with a progress line, and
  asks again when the index is done if it is still open.

## Text search

Text search runs in the core on `grep-regex` and `grep-searcher`. It reads the Lookup checkout for the base
and the mirror for the PR's files, and offers match case, whole word and Rust's regex syntax. It waits while
the checkout moves, and has no `git grep` fallback.

## What the spike measured

On CPython main at `ce4ac8c045`, on Windows, with ty 0.0.103 and clangd 24.0.0git (snapshot of 2026-09-13):

- The first checkout took 1.7 s for 5,971 files, and a move 0.4–0.7 s.
- ty answered `initialize` in 11–22 ms and its first lookup in about 0.1 s, then in milliseconds, using
  about 65 MB. Locals, parameters and `self` were right, and standard library names landed in the PR's
  `Lib`, even modules that exist only on main. Names written in C land on typeshed's stubs.
- clangd answered `initialize` in about 55 ms and its first lookup in about 4 s. It indexed 452 files in
  4 min 9 s, after which 13 of 13 lookups were right, and caught up after a move in 11 s. It held
  0.4–0.5 GB, and 0.67 GB at the peak of its first index build.
- Switching views took about 0.4 s in clangd and 25 ms in ty.
- Search: `git grep` took 1–1.8 s on a regex such as `.*GetAttr`, and ripgrep stayed near 150 ms.

## Considered Options

- **`git grep` with a regex for each language's definitions**, which v1 first planned. It can't tell a
  definition from a word that looks like one, and every language needs patterns of its own.
- **tree-sitter's tags queries**, which the grammar crates ship for GitHub's code navigation. They find lines
  that define something, but have no scopes: every `self` in the codebase matches.
- **A scope resolver on tree-sitter's trees.** It would follow each language's rules, which means writing
  part of a language server for each language.
- **Servers reading the mirror's objects, with no checkout.** ty and clangd resolve imports and includes on
  disk, so they need files there.
- **The Review checkout as the Lookup checkout.** It moves only when the user asks and the user's editor
  writes to it, so lookups would answer from whatever it holds.
- **Closing the PR's files to get the base view.** ty handles it, but clangd keeps the closed text.
- **A single `compile_flags.txt` for clangd.** clangd never starts a background index with it, so a
  definition in another file stays on its header declaration.
- **Downloading and pinning servers, as Zed does.** It is more code to write and maintain, and users who
  review CPython already have these tools or can unpack an archive.
- **`git grep` as a fallback while the Lookup checkout moves.** Its regex dialect and its idea of a word
  differ from the engine's, so one query would match differently depending on timing. The wait it would
  save is about a second once per PR.

## Consequences

- This replaces the symbol lookup consequence of
  [ADR 0002](0002-tsugiki-owns-its-git-state.md). That ADR deferred language servers for the lack of a
  `compile_commands.json` on Windows, a `pyconfig.h` for each branch, and 1–2 GB of memory. Tsugiki writes
  its own compile database, the Lookup checkout brings the branch's `PC/pyconfig.h`, and clangd held under
  0.7 GB.
- Each Repository now holds two worktrees, and the Lookup checkout moves on every PR that opens. A Repository
  that is removed takes both with it.
- The LSP client is a crate of its own, `crates/lsp` (`tsugiki-lsp`), which knows nothing of PRs or
  Mirrors. `core` decides which server to start, what to open and what to ask.
- Go to definition needs a server for the file's language. Without one, the peek says how to set it up.
- Names written in C and used from Python land on typeshed's stubs, not on the C code that implements them.
