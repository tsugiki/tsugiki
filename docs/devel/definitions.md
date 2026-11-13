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
