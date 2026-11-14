# Go to definition

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A click on a word in the Files tab's diff puts the caret on it, and a double-click selects it. The picked
  word is highlighted, but its other matches in the diff aren't. A word is a run of letters, digits and
  underscores. Any line of code from the **Mirror** takes a pick: added, removed, unchanged or expanded, in
  either view. A thread's snippet on the Conversation tab doesn't. A click on no word drops the pick, and a
  drag still selects text to copy.

- **Go to definition** (`F12`) and **Definition peek** (`Alt+F12`) ask a language server where the picked
  word is defined: ty for Python, clangd for C and rust-analyzer for Rust
  ([ADR 0020](../adr/0020-go-to-definition-asks-a-language-server-which-reads-a-lookup-checkout.md)). The
  server reads the **Lookup checkout**, which holds the PR's base and its added files, and sees each file
  the PR changes as an open document. A removed line asks at the base, and every other line at the PR's head.
  A lookup never runs the PR's code. A definition in a file the PR changes is shown in the diff; elsewhere,
  Go to definition opens that file read-only at the same commit in the Detail pane, and `Alt+←` returns to
  the diff.
- The peek shows the server's answer, or "No definition of X". While clangd builds its index, a definition
  in another file lands on its declaration in a header: the peek shows it with a progress line, and asks
  again once the index is done if it is still open.
- The user supplies each server, picks its binary in Settings, and Tsugiki keeps the path in `machine.toml`.
  Tsugiki checks a server when its path is set, names the versions we tested, and warns about another
  version but still uses it. A server's problem shows in its row in Settings with its log, and in the peek in
  place of "No definition of X". Without a server for the file's language, the peek says how to set one up.
- ty and clangd start when a PR that touches their language opens, and rust-analyzer on the first lookup in
  Rust. Each stops after 30 minutes idle, which `machine.toml` can change (see "When a server stops").
- Text search is a separate action, on ripgrep's crates, over the Lookup checkout and the PR's files, with
  match case, whole word and Rust's regex syntax. It waits while the checkout moves.

## The Definition peek in Python

- `Alt+F12` on the picked word opens the Definition peek under its line, and the rows after it move down.
  The core asks ty where the word is defined, through `tsugiki-lsp`, off the window's thread. Esc or ✕
  closes the peek and gives the focus back to the diff.
- Until Settings can pick it (#683), ty's binary comes from `[language_servers.ty] path` in `machine.toml`.
  A relative path starts at `machine.toml`'s folder. We test with ty 0.0.103, which `just test-servers`
  fetches. The tests read its path from `TSUGIKI_TY`, which `just cargo-test` and `just rust-coverage` set.
- Each Repository gets its own ty. It starts when a PR that changes a `.py` or `.pyi` file opens, once the
  Lookup checkout is at that PR, and never when Tsugiki starts. A lookup that comes first starts it too. A
  PR at other commits gets a new ty, as the checkout has moved, and Tsugiki stops each ty when it quits.
  Each ty runs with the checkout as its working folder.
- In a Repository with CPython's layout, recognised by `Include/patchlevel.h`, ty gets `environment.root =
  ["Lib"]` and the Python version that file defines, through `workspace/configuration`. Elsewhere ty gets
  no settings, and reads the Repository's own.
- The PR's changed Python files open on ty with their text at the head, read from the **Mirror**, and stay
  open while ty runs. A lookup from a removed line first gives them their text at the base through
  `didChange`, and the next lookup from another line puts the head's back. A file the PR adds keeps its
  head text, and one it deletes isn't open, so ty reads the base's from the checkout. A removed line of a
  renamed file asks at its old path, which the checkout holds at the base, and its new path keeps its head
  text either way.
- The Files tab reads its PR again on each change, and only a PR at other commits starts ty again. A ty
  that failed to start, or a checkout that failed to move, waits for the next lookup to try again.
- The peek opens as soon as the user asks and says what the lookup waits for: "Starting ty…" while ty
  starts, then "Asking ty for the definition of X…", with the seconds once they pass one. The core says
  which on a channel the `definitions` command takes. A lookup has no time limit: clangd's first answer on
  CPython takes about 4 s, and ty may read for longer on its first lookup in a big Repository.
- A new `F12` or `Alt+F12` while a lookup runs cancels it with `$/cancelRequest`, and the window drops
  its answer. Esc while the peek waits cancels the lookup and closes the peek, and so does a newer diff.
  The window gives each lookup a ticket, and a cancel names it: a cancel the core takes before its lookup
  still cancels it, and a late one for a lookup that has answered can't cancel the next. A newer lookup or
  a cancel that comes while ty starts means the core asks ty nothing for the older one. ty goes on reading the
  Repository either way. The servers' thread sends ty each request and goes on, and a thread of the
  lookup's own waits for ty's answer and hands it back, so a cancel never waits behind a lookup.
- Each hit names its commit: a file the PR changes, asked from a line at the head, is at the PR's head, and
  so is a file the PR adds or renames, whichever line asked. Any other file of the Repository is at the
  base. A hit outside the Repository, such as a stub of the
  standard library that ty keeps, names its path on disk. The peek's header shows the symbol, the first
  hit's path and line, "at abc1234 · the PR's head" or "the PR's base", "1 of N" and ✕. A hit outside the
  Repository shows its whole path on disk in place of the commit.
- Under the header the peek shows the hit's file, with line numbers in the gutter, scrolled so the hit's
  line sits 3 rows from the top, and marked. A file of the Repository comes from the **Mirror** at the hit's
  commit, so a hit in a file the PR changes, found at the head, shows the head's text. A file outside it
  comes from disk, and only one a server named in a hit can be read. The lines are plain text until #455.
  The peek takes 18 rows while it looks up or shows code, and 2 when it only has words to say. Its code
  scrolls inside it, so the diff's rows after it stay where they are.
- The window asks the core for the file 200 lines at a time near the rows that show, so a big file such as
  CPython's `Objects/unicodeobject.c` opens without waiting for all of it. The core reads them on the
  command's thread, never waiting for a lookup, and offline at any commit the Mirror holds. It keeps the
  last file it read, so the next chunk starts no git. A line ends at a lone `\r` too, as LSP counts lines.
  A chunk that fails once lines show leaves them, and the next scroll asks again.
- LSP counts a line's columns in UTF-16 code units, and so does the webview, so the picked word's column
  goes to ty as it is, and a hit's column comes back the same way.
- With no hit the peek says "No definition of X", and with no ty set up "Set up ty to go to definitions in
  Python". A lookup in a file of another language says Tsugiki can't go to definitions in it yet. A ty that
  can't start, or fails a lookup, says why in the peek, and the next lookup starts a new one.
- ty reads the checkout and the Mirror, so a lookup works offline at any commit the Mirror holds.

## Go to definition in Python

- `F12` on the picked word, or `Ctrl+click` on a word (`Cmd+click` on macOS), runs the lookup `Alt+F12`
  runs, at the same commit. While it looks, the line over the diff says "Searching for the definition of
  X…", a peek on another word closes, and Esc stops it.
- It goes to the first of ty's hits on a line a hunk shows: the diff scrolls there, opening the file if it
  is collapsed, and marks the line for a moment. A hit at the head lands on the line's new side, and one at
  the base on its old side, which side by side marks only that half. The core finds the row in the diff on
  show, from the hit's path and line at its side, the old path for a renamed file at the base.
- With no hit, the line over the diff says "No definition of X at abc1234." for 4 seconds.
- Until the later tickets land, a hit on no line of the diff opens the peek on it: a line no hunk shows
  (#445), or a file the PR doesn't change (#453). So does an answer with no hit to go to, which says why. With
  several hits it goes to the first that lands, until #446 opens the peek with the list.
- A development build gives `F12` to Go to definition too, and opens DevTools on `Ctrl+Shift+I` or from the
  palette.

## Go to definition in Rust

- Go to definition and the Definition peek work in `.rs` files as they do in Python, through rust-analyzer.
  The PR's changed Rust files open on it and switch views as Python's do on ty.
- Until Settings can pick it (#683), rust-analyzer's binary comes from `[language_servers.rust-analyzer]
  path` in `machine.toml`. It has to be the one inside a Rust toolchain, which `rustup which rust-analyzer`
  prints: rust-analyzer then reads the standard library from that toolchain's `rust-src`, and runs its cargo
  and rustc. One outside a toolchain, such as rustup's proxy in `~/.cargo/bin`, doesn't start and the peek
  says why, as the checkout's files could then pick the programs it runs.
- We test with rust-analyzer 1.99.1 (58cf8f9c 2026-10-13), the rustup component of the Rust 1.99.1 that
  `rust-toolchain.toml` pins, which installs it with `rust-src`. The tests read its path from
  `TSUGIKI_RUST_ANALYZER`, which `just cargo-test` and `just rust-coverage` set from `rustup which`.
- rust-analyzer starts on the first lookup in a `.rs` file, never when a PR opens. It finds nothing in a file
  until it has loaded the workspace, so the core waits until rust-analyzer says it is quiescent, for at most
  30 s, while the peek says "Starting rust-analyzer…". On Tsugiki's own repository that first lookup takes
  about 9 s, and each lookup after it a few milliseconds. The servers' thread waits too, so a cancel or a
  lookup in Python waits behind that start, as it does behind ty's.
- A PR at other commits stops its Repository's rust-analyzer and cancels a lookup that waits on it, as the
  checkout moves. The next lookup in Rust starts a new one.
- A lookup never runs the Repository's code. rust-analyzer runs no build script, builds no proc macro, and
  runs no `cargo check` or diagnostics. Cargo works offline, so it downloads nothing, and resolves the
  dependencies its cache holds. A dependency it lacks leaves lookups in the workspace and the standard
  library working.
- What rust-analyzer starts runs with the toolchain rust-analyzer is in: `RUSTUP_TOOLCHAIN`, `CARGO` and
  `RUSTC` name it, whatever toolchain the checkout's `rust-toolchain.toml` names, and `RUSTUP_AUTO_INSTALL=0`
  stops rustup installing one. An empty `RUSTC_WRAPPER` beats a wrapper the checkout's `.cargo/config.toml`
  names.
- rust-analyzer 1.99.1 reads a `rust-analyzer.toml` at the checkout's root. It refuses `procMacro.enable`
  there, and a `cargo.buildScripts.enable = true` there builds nothing, which a test checks. It also reads
  the user's own `rust-analyzer.toml` in their config folder.

## Go to definition in C

- Go to definition and the Definition peek work in `.c` and `.h` files as they do in Python, through clangd.
  The PR's changed C files open on it and switch views as Python's do on ty.
- Until Settings can pick it (#683), clangd's binary comes from `[language_servers.clangd] path` in
  `machine.toml`. clangd reads its built-in headers from `lib/clang` beside its `bin` folder, so it stays in
  the folder its release unpacks to. We test with clangd 23.1.0, which `just test-servers` fetches whole. The
  tests read its path from `TSUGIKI_CLANGD`, which `just cargo-test` and `just rust-coverage` set.
- Each Repository gets its own clangd, which starts as ty does: when a PR that changes a `.c` or `.h` file
  opens, or on a lookup that comes first. A PR at other commits gets a new one. It runs with
  `--background-index --background-index-priority=low -j=4`.
- In a Repository with CPython's layout, recognised by `Include/patchlevel.h`, Tsugiki writes clangd a
  `compile_commands.json` into `repos\<owner>__<name>\servers\clangd\`, beside the Lookup checkout, and
  starts clangd with `--compile-commands-dir` set to it. It has one entry for each C file under `Objects/`,
  `Python/`, `Parser/`, `Modules/` and `PC/` in the checkout, which holds the PR's added files too, and
  leaves out the vendored `libmpdec`, `expat` and `_hacl`, `Objects/mimalloc`, which builds only as part of
  `Objects/obmalloc.c`, and `Python/bytecodes.c`, the source of the generated interpreter cases. Each entry
  gets the flags CPython's Windows build gives `pythoncore`: `-xc -std=c11
  --target=x86_64-pc-windows-msvc`, `Include`, `Include/internal`, `Include/internal/mimalloc` and `PC` as
  include folders, and `WIN32 _WIN32 _WIN64 _M_X64 Py_BUILD_CORE Py_BUILD_CORE_BUILTIN Py_ENABLE_SHARED`.
  Each start writes it again, so each move does too. clangd keeps its index in `.cache\clangd\index\` in the
  same folder, outside the checkout.
- Elsewhere clangd uses its own defaults, and finds a `compile_commands.json` the Repository holds. Without
  one it indexes nothing, so it finds definitions only in the file and the headers it includes.
- Nothing is configured or built: clangd reads the flags Tsugiki wrote, and runs no compiler to ask for its
  include folders unless `--query-driver` allows it, which Tsugiki never passes.
- Only the flags for Windows are known. macOS and Linux need the `pyconfig.h` that `configure` generates, so
  they wait for the macOS checks before 1.0.
- Until clangd's index holds the file that defines a function, a lookup lands on its declaration in a
  header. clangd 23.1.0 indexed the 453 files of CPython's core at `ce4ac8c045` in 80 s, after which
  `PyErr_Format` in `Objects/typeobject.c` landed in `Python/errors.c`.

## When a server stops

- A server that no lookup has reached for 30 minutes stops: Tsugiki sends `shutdown`, then `exit` once the
  server agrees. `[language_servers] idle_minutes` in `machine.toml` changes the time, and a value that
  isn't a whole number above 0 counts as unset. A lookup that runs keeps its server, and its answer counts
  as a use. The next lookup starts the server again: clangd's index stays on disk, so it doesn't index from
  scratch. Opening the same PR again starts nothing.
- Quitting Tsugiki stops every server the same way, and waits for them within the second it gives all its
  threads to stop. A server that hasn't agreed 5 s after `shutdown`, or hasn't exited 5 s after `exit`, is
  killed.
- On Windows each server runs in a job object that kills the server and the processes it started when
  Tsugiki's process ends, so a crash leaves no server behind. `crates/lsp/tests/crash.rs` kills a parent
  process that stands in for Tsugiki. Off Windows a crash only closes the server's input, which waits for
  the macOS checks before 1.0.
- Removing a Repository stops its servers, cancels a lookup that waits on one, and waits until the servers'
  own processes have ended: the core's `Definitions::remove`, which #543 calls. A process a server started,
  such as rust-analyzer's `cargo`, may still be ending then, so a delete of the folder retries. Switching
  to another Repository leaves them to the idle time.

## Text search

- The core searches the open PR's code at its head, on ripgrep's `grep-regex` and `grep-searcher`. It reads
  the files the PR doesn't change and the ones it adds from the **Lookup checkout**, and the head's text of
  each file it changes from the **Mirror**. The files it deletes aren't searched. Links and submodules hold
  no text, so they aren't either. It needs nothing but the Mirror and the checkout, so it works offline at
  any commit the Mirror holds.
- A query is a literal that matches whatever the case, inside a longer word too, as in VS Code. Match case,
  whole word and regex each change that, and a regex reads as Rust's regex crate reads it. A line ends at a
  line feed, and `$` matches before a `\r\n` too. A regex that doesn't parse returns the regex crate's
  error and nothing runs, and the query running goes on.
- A file that holds a NUL byte is binary and skipped, as git and ripgrep skip one. A Generated file is
  searched, and its results say it is one: the `.gitattributes` in the Lookup checkout mark it
  `linguist-generated`. Those are the base's and any the PR adds, where the Files tab reads the Target
  branch's, as an index of the base to read them from would take longer than the search. git is asked about
  each batch of files that hold a match, so the first results don't wait on the whole tree.
- Each result is a file's path with each line that holds a match: its number, its text and where each match
  is in UTF-16 code units, as the webview counts. A line longer than 250 characters shows 250 of them,
  starting 50 before its first match unless the line ends sooner, and says which ends it cut. A match that
  doesn't fit isn't shown and doesn't count.
- The results reach the window in batches as files finish, in order of paths: each batch holds the files
  that finished while git said which of the last batch's are generated. A
  search stops at 5,000 matches and says so. The files that make up those 5,000 are the ones read first,
  which can differ from one run to the next. A new query cancels the one running: it starts no more git,
  reads no more files, and what it found doesn't reach the window. Nothing holds the checkout still during a
  search, as it moves only when another PR opens, and the window then asks a new query.
- A query that arrives while the Lookup checkout moves waits for the move, and after 100 ms it says that it
  waits. With no Lookup checkout, as when its last move failed, it says there is nothing to search.
- It runs off the window's thread, on up to 4 threads: 3 read files from disk and one reads the PR's files
  from the Mirror. On CPython's main branch a whole-tree search takes about 0.2 s once the files are in the
  disk's cache.
