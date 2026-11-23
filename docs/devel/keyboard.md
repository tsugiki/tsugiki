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
| Open the files unchanged since a Stale review | `Ctrl+K J`, or the command palette |
| Show or hide the rail's Files | `Ctrl+K F`, or the command palette |
| Search the PR's code in the rail | `Ctrl+Shift+F`, or the command palette |
| Next / previous tab in the PR view | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Next / previous file in the Files tab | `Alt+PageDown` / `Alt+PageUp` |
| Send comment | `Ctrl+Enter` |
| Open in editor | `Ctrl+Shift+E` |
| Local mark | `Ctrl+K M` |
| Pin or unpin | `Ctrl+K P` |
| Pick a theme | `Ctrl+K T`, or the command palette |
| Reload themes | `Ctrl+K R`, or the command palette |
| Back / forward | `Alt+←` / `Alt+→` |
| Open on GitHub | `Ctrl+Shift+G` |
| Switch Repository | `Ctrl+R`, or the command palette |
| Go to the Home page | the command palette, or the Repository picker's first row |
| Reset the layout | the command palette |
| Archive, the row under the cursor or the Detail pane's item, by focus | `E` |
| Mute, the row under the cursor or the Detail pane's item, by focus | `M` |
| Mute or unmute the author of the row under the cursor or of the Detail pane's item, by focus | the command palette |
| Mark seen / unseen, the row under the cursor or the Detail pane's item, by focus | `Q` / `Shift+Q` |
| Settings | `Ctrl+,`, or the command palette |
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
- `Ctrl+K F` opens the rail beside the PR's tabs on its Files tab, and closes it while Files shows
  ([review.md](review.md)). Each of the rail's tabs has a key of its own that does the same. They work on
  either of the PR's tabs, and with the focus in the rail. ← → move between the rail's tabs while its tab
  row holds the focus, and Home and End go to the first and the last. In the Files tab's filter box, ↓
  goes to the tree of files, where ↑ ↓ move through the rows, ← folds a folder and → unfolds it, and
  Enter opens a file in the Files tab.
- `Ctrl+Shift+F` opens the rail on Search with the focus in its query box, as in VS Code, and works in a
  text box too. Pressed with Search on show, it focuses the box and leaves the rail open. In the box,
  `Alt+C`, `Alt+W` and `Alt+R` turn match case, whole word and regex on or off, as VS Code's keys do. ↓
  goes from the box to the results, where ↑ ↓ move through the rows, ← folds a file and → unfolds it,
  and Enter opens the match. These keys belong to the box and the list, so `[keys]` can't remap them.
- `Ctrl+K J` opens the files of a Stale review's Files tab that the Review left unchanged, as VS Code's
  `Ctrl+K Ctrl+J` unfolds all its regions.
- `Ctrl+P` opens quick open, which finds the open Repository's Issues and PRs in the database. The user
  types a number, as `123`, `#123` or `gh-123`, or words from a title, and Enter opens the item in the
  Detail pane, on its line in the Issue list if the view on show has one. The PR on show stays on its tab.
  The item with that number comes first. Then come the items whose titles hold every word typed, in any
  case: the open ones first, each group most recently updated first. So `gh-123` also finds the PRs whose
  titles name Issue 123. Nothing typed lists every item in that order. Quick open lists 20 at most. An
  Enter pressed before the answer to the text in the box opens the first item of that answer, unless the
  user types on or moves the pick first.
- A number that names nothing the database holds lists one row first, "Fetch #123 from GitHub", in place
  of the item. Enter on it fetches the Issue or PR from GitHub into the Detail pane, whichever kind the
  number turns out to be ([layout.md](layout.md)). The search reads only the database, and only Enter on
  that row asks GitHub. A number the database holds lists no such row. While the status bar says GitHub
  can't be reached, or GitHub's rate limit holds every request until the reset ([sync.md](sync.md)), the
  row reads "Open #123 in browser" instead, and Enter on it opens the number on GitHub at once, as a fetch
  would only wait for its failure.
- With a PR on show, quick open lists the files it changes before the Issues and PRs, after only the item
  with the number typed: the files whose paths hold every word typed, in any case, in the order the Files tab
  shows them, 20 at most. Nothing typed, or a text that starts with `#`, lists no files, and reads no diff.
  Other text reads the PR's diff if the Files tab hasn't yet, and an Enter pressed before the core first
  answers that read waits for it, as it waits for the Issues and PRs, unless the item with the number typed
  heads the list or the user picked one of the rows on show with the arrows. A diff that doesn't come at
  once, as while its head is fetched, lists no files. The files show when the search of the Issues and PRs
  fails too, above why it failed. Enter opens the file in the Files tab from its top, opening it if it is
  collapsed: with all the files the tab scrolls to its header, and one file at a time it shows that file. The
  file view over the tab closes, and the diff takes the focus. A file the diff no longer changes, as after a
  sync moved the PR's head, is said over the diff for a moment. With no PR on show, or once the PR leaves the
  Detail pane, quick open lists no files.
- Quick open and the command palette share one box, as in VS Code: `>` in front lists the commands, and
  `Ctrl+Shift+P` opens the box with `>` typed. Quick open searches one Repository, so on the Home page
  `Ctrl+P` does nothing and the command palette has no `>` to delete.
- `?` lists the keys that work where the focus is, each with its action's title and the key in force: the
  Issue list's while the list has the focus, the diff's while the Files tab shows its diff, then those that
  work anywhere. A key is listed only where it would run its action: Next Tab while a PR shows, `Q` and
  `Shift+Q` while they have an item to mark, and in a text box only the keys that work there. Esc closes the
  list, and in a text box `?` types.
- The command palette lists each action of the key table that can run where the focus was as it opened, in
  the table's order, each with its key in force on the right, remaps included, or no key. Running one does
  what its key does there, once the palette has handed the focus back, and in a text box too, where the box
  would keep the key. So the Issue list's palette has Unfold and Fold, and the diff's has Next Change and Find
  in Diff, and Mark Seen marks what `Q` would. An action with nothing built behind it yet, such as Local
  Mark, isn't listed, and neither are the palette's own two keys, which open the box it is. A command the
  palette had before keeps its title, such as Hide Sidebar, which turns into Show Sidebar, or View: Files in
  the Rail. The commands that run no action, such as Sort by Attention and a switch to each other
  Repository, come after the actions.
- The command palette lists the commands the user ran at the top, newest first, then those never run, in
  their usual order. Typing filters the list by substring and keeps that order. `machine.toml` keeps the
  last 50 commands run, newest first, in `recent_commands` beside the recent Repositories, so the order
  survives a restart and stays on its machine. A command whose title flips, such as Hide Sidebar and Show
  Sidebar, keeps one place under both titles. The palette's rows that switch to a Repository stay out of the
  history and come most recently opened first, as the Repository picker lists them.
- Settings › Keyboard lists every action in a table, in the order of the core's key table: its title, its key
  in force, or No key, and where the key works, Anywhere, Issue list, Diff or Composer. An action `[keys]`
  gives another key than its default, or none, is marked remapped, with its default in the mark's tooltip;
  a line that gives an action its own default isn't marked. An action whose key another action's line took,
  its default or the key its own line gives, shows No key and says which action has the key now. A box
  above the table filters its rows by what they hold, in any case: an action's title or its name in
  `[keys]`, its key as the page shows it, No key included, or as `[keys]` writes it, so "ctrl+k" finds
  `Cmd+K M` on macOS, or the note on a key another action took. Esc in the box clears it, and closes
  Settings once the box holds nothing but spaces; an Esc that cancels an input method's candidates leaves
  the box alone. The page says the
  defaults follow VS Code, and that `Cmd` stands for `Ctrl` on macOS.
- Keybindings can be remapped on the Settings Keyboard page: click a key and press the new one; chords such as
  `Ctrl+K M` work, and two actions on one key are flagged. Only remapped keys are written to the portable
  config file.
- `[keys]` in `settings.toml` holds one line per remapped action, named as the core's key table names it:
  `archive = "a"`, `local_mark = "ctrl+k m"`, or `""` for no key. A key is written as the window names it,
  with Ctrl, Alt and Shift before the key and a space between a chord's two presses, in any case: `"Ctrl+K M"`
  reads as `"ctrl+k m"`, `Cmd` and `Option` read as `Ctrl` and `Alt`, and `←` reads as `arrowleft`. Tsugiki
  reads the table at start-up and when Settings opens ([ADR
  0014](../adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)). A remapped key
  works wherever its action does, and every tooltip, palette note and key hint shows it.
- A line of `[keys]` that names no action, gives a key Tsugiki can't read, or puts an action on a key that
  can't be remapped shows in the status bar with its line, and the action keeps its default key. The arrows,
  Enter, Esc, Tab, Space, Home, End, Page Up and Page Down can't start a key without Ctrl or Alt, unless the
  line gives the action its own default, as `unfold = "arrowright"` does; a chord's second press can be any
  key. Shift with a symbol or a digit is refused with the character to write instead, `"?"` for
  `"shift+/"`, as the window names such a press by what it types. A key the window names by where it is, on
  a non-Latin layout or with macOS's Option, goes by what Shift types there on a US keyboard: Russian
  `Ctrl+Shift+Б` is `"ctrl+<"`, and `Option+Shift+/` is `"alt+?"`, as Windows names `Alt+Shift+/`. Open
  DevTools can't be remapped, and no key can hold `Ctrl+Shift+I`: WebView2 opens DevTools on it. A
  plain key, Backspace and Delete still type in a text box, and so does `Ctrl+Alt` with a key that types, as
  AltGr sends it, unless the action works in a text box, as Hide or Show the Sidebar does. The Settings
  dialog's Apply leaves `[keys]` as the user wrote it.
- Any action can take a chord or a key of one press, and a chord runs the action as its key of one press
  would, in the Definition peek too. No chord starts under a dialog or the palette. A chord of an action the
  window runs itself, such as `open_settings = "ctrl+k s"`, works on every screen and in a text box; the
  chords of the Repository screen's actions work there, outside a text box.
- Until two actions on one key are settled (#550), a remapped action keeps its key over an action that has
  it by default, which is left with no key: after `archive = "f7"`, F7 runs Archive, and Next Change shows no
  key. Of two remapped actions on one key, the later in the key table keeps it, and the other shows no key.
