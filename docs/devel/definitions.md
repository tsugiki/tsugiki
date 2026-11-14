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

## The Definition peek in Python

- `Alt+F12` on the picked word opens the Definition peek under its line, and the rows after it move down.
  The core asks ty where the word is defined, through `tsugiki-lsp`, off the window's thread. Esc or ✕
  closes the peek and gives the focus back to the diff.
- Until Settings can pick it (#683), ty's binary comes from `[language_servers.ty] path` in `machine.toml`.
  A relative path starts at `machine.toml`'s folder. We test with ty 0.0.103, which `just fetch-ty` fetches.
  The tests read its path from `TSUGIKI_TY`, which `just cargo-test` and `just rust-coverage` set.
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
  that failed to start, or a checkout that failed to move, waits for the next lookup to try again. A lookup
  ty hasn't answered in 30 seconds is cancelled, and ty goes on reading the Repository.
- Each hit names its commit: a file the PR changes, asked from a line at the head, is at the PR's head, and
  so is a file the PR adds or renames, whichever line asked. Any other file of the Repository is at the
  base. A hit outside the Repository, such as a stub of the
  standard library that ty keeps, names its path on disk. The peek's header shows the symbol, the first
  hit's path and line, "at abc1234 · the PR's head" or "the PR's base" (or "outside the Repository"), "1 of
  N" and ✕, and the line under it shows the hit's line until #439 shows its code.
- LSP counts a line's columns in UTF-16 code units, and so does the webview, so the picked word's column
  goes to ty as it is, and a hit's column comes back the same way.
- With no hit the peek says "No definition of X", and with no ty set up "Set up ty to go to definitions in
  Python". A lookup in a file of another language says Tsugiki can't go to definitions in it yet. A ty that
  can't start, or fails a lookup, says why in the peek, and the next lookup starts a new one.
- ty reads the checkout and the Mirror, so a lookup works offline at any commit the Mirror holds.
