# Keyboard

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- Every action can be done from the keyboard. Default keybindings follow VS Code, and on macOS `Cmd` replaces
  `Ctrl`.

| Action | Key |
|---|---|
| Command palette | `Ctrl+Shift+P` |
| Quick open | `Ctrl+P` |
| Go to definition / Definition peek | `F12` / `Alt+F12` |
| Next / previous change | `F7` / `Shift+F7` |
| Next / previous comment thread | `F8` / `Shift+F8` |
| Find in diff | `Ctrl+F` |
| Hide / show the Sidebar | `Ctrl+B` |
| Hide / show the Issue list | `Ctrl+Alt+B` |
| Unfold / fold an Issue's PRs in the Issue list | `→` / `←` |
| Show only the Detail pane / show the panes | `Ctrl+K Z` |
| Unified ↔ side-by-side | `Ctrl+Alt+U` |
| All files ↔ one file at a time | `Ctrl+K O`, or the command palette |
| Next / previous tab in the PR view | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Next / previous file in the Files tab | `Alt+PageDown` / `Alt+PageUp` |
| Send comment | `Ctrl+Enter` |
| Open in editor | `Ctrl+Shift+E` |
| Local mark | `Ctrl+K M` |
| Pin or unpin | `Ctrl+K P` |
| Pick a theme | `Ctrl+K T`, or the command palette |
| Back / forward | `Alt+←` / `Alt+→` |
| Open on GitHub | `Ctrl+Shift+G` |
| Switch Repository | `Ctrl+R`, or the command palette |
| Reset the layout | the command palette |
| Archive / Mute the selected item | `E` / `M` |
| Mark seen / unseen, the row under the cursor or the Detail pane's item, by focus | `Q` / `Shift+Q` |
| Settings | `Ctrl+,` |
| Zoom in / out / reset | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |
| Show or hide the performance HUD | `Ctrl+Shift+H` |
| Open DevTools, in a development build only | `Ctrl+Shift+I`, or the command palette |

- The webview's own browser shortcuts (Print, Reload, Find, zoom, DevTools) never fire in a release build;
  Tsugiki's keys win, on any keyboard layout. Zoom is Tsugiki's, kept per machine, and Ctrl+wheel doesn't
  zoom. On macOS the standard menu stays, with Settings… on `Cmd+,`; a remap onto one of its shortcuts is
  refused ([ADR 0013](../adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)).
- `Ctrl+K O`, "one", switches the Files tab between all files and one file at a time. It is a `Ctrl+K`
  chord, as `Ctrl+K Z` is for the panes, because a free `Ctrl+Alt` letter would clash with AltGr, which
  Windows sends as `Ctrl+Alt` to type characters such as Polish ó on `AltGr+O`. VS Code's own `Ctrl+K O`
  opens the active file in a new window, which Tsugiki has no use for.
- The command palette's quick open finds Issues, PRs, and files in the current PR. `?` lists the keys
  available in the current view.
- Keybindings can be remapped on the Settings Keyboard page: click a key and press the new one; chords such as
  `Ctrl+K M` work, and two actions on one key are flagged. Only remapped keys are written to the portable
  config file.
