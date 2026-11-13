# Go to definition

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A click on a word in the Files tab's diff puts the caret on it, and a double-click selects it. The picked
  word is highlighted, but its other matches in the diff aren't. A word is a run of letters, digits and
  underscores. Any line of code from the **Mirror** takes a pick: added, removed, unchanged or expanded, in
  either view. A thread's snippet on the Conversation tab doesn't. A click on no word drops the pick, and a
  drag still selects text to copy.

- **Go to definition** (`F12`) and **Definition peek** (`Alt+F12`) find a symbol's definition with `git grep`
  at the line's commit: the PR's head, or the base for a removed line. They know Python, C in CPython's style
  and the reST directives in `Doc/`, and fall back to a whole-word search. A definition in a file the PR
  changes is shown in the diff; elsewhere, Go to definition opens that file read-only at the same commit in
  the Detail pane, and `Alt+←` returns to the diff ([ADR 0002](../adr/0002-tsugiki-owns-its-git-state.md)).
