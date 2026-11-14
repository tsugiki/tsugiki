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
  Rust. Each stops after 30 minutes idle, which `machine.toml` can change.
- Text search is a separate action, on ripgrep's crates, over the Lookup checkout and the PR's files, with
  match case, whole word and Rust's regex syntax. It waits while the checkout moves.

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
