# Settings

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- **Settings** is a dialog over the app, opened with `Ctrl+,`, ⚙ in the Sidebar or on the Home page, or the
  command palette. Its left column lists the pages (General, Account, Repositories, Sync, Keyboard, Advanced),
  with each Repository under Repositories as a page of its own. Repositories itself is an overview with Add
  repository. A Repository's page holds its conventions, first sync, the clone to borrow objects from, Filters
  and Muted authors; Pinned issues are not there, since the list can be long: they are managed from the
  Sidebar's Pinned issues (★ on an item, `Ctrl+K P`). The dialog moves by its title bar, resizes from any
  edge or corner, and can be maximized; its size and place are kept per machine. The Account page is in
  [token.md](token.md). This was chosen out of three variants prototyped on branch `prototype/app-shell`: a
  searchable list like VS Code's, this dialog, and editing the config files as text.
- The dialog opens on General, or on Advanced from the Definition peek. As it opens it reads both files
  again, so a hand edit made since start-up shows in it and takes effect on the window, the theme included.
  The controls wait until then; if the files can't be read, they stay off and the dialog says why. A
  Repository's new Conventions or Filters from the edit reach its lists at once, and its syncs. Each change
  shows on the window at once: clearing Show the status bar hides the status bar behind the dialog.
  Apply saves the tables the dialog changed over what the file says then, so a hand edit to another table
  made while the dialog was open stays, and closes; with nothing changed the button reads OK and closes
  without saving. Cancel, Esc, ✕ or a press outside the dialog puts every change back. While Apply saves,
  Cancel and Esc wait for it, and a save the core refuses keeps the dialog open with the reason under the
  pages, where Cancel still puts the changes back.
- Each setting carries a label for where it is stored: settings file, this machine, database or keychain,
  with the file's name or "on this machine only" in its tooltip.
- General groups its settings; each group is a heading with its rows under it. Appearance comes first,
  with three rows ([#757](https://github.com/tsugiki/tsugiki/issues/757)), as VS Code keeps
  `workbench.preferredLightColorTheme` and `workbench.preferredDarkColorTheme` in its settings. Theme lists
  Follow system, then the light Themes, then the dark ones, as the theme picker lists them, and saves
  `theme`. Light theme and Dark theme list the Themes of their side, save `light_theme` and `dark_theme`,
  and say under the row that Follow system shows them. The files that can't be used aren't listed. A value
  the file holds that a row doesn't list shows as an option of its own: "<name> (not found)", "<name> (can't
  be used)" for a file that failed, "<name> (a dark theme)" in Light theme, and a mode from a file written
  before as "Always the Light theme" or "Always the Dark theme". It stays among the options after a
  change, so the user can go back to it. Apply sends only the keys of `[appearance]` that changed, so
  another change leaves such a value in the file, and so does a hand edit to another key, and it refuses a
  Theme picked by `system`, `light` or `dark`. A change shows on the window at once, title bar included:
  `preview_themes` works out the Themes the rows give by the rules `themes` goes by for the file, and the
  window turns to their side, so Follow system shows the side the system is on, not the side a Theme kept
  the window on. The window sends one `preview_themes` at a time, and the next carries the rows' latest
  values. The rows are off while Apply saves, which keeps the Themes of the values it saved in force and
  reads the themes again for the files' problems. If the window has none to keep, it goes on showing what
  it shows while Apply reads them. When it can't, Apply stays open with why, and the window shows the
  Theme in force from the last read that worked, as after any read that fails, so OK and Cancel have
  nothing to put back. Cancel turns the window back. When the window has no themes at all, the rows stay
  off and say why. Status bar has Show the status bar, and says that problems and being
  offline still show when it's off. Updates has Check for new versions (`[releases] check`), the one setting
  that still needs a restart, so it says "Takes effect at the next start" under its row. Both live in
  `settings.toml`. Sync says "Nothing to set here yet." for now.
- Repositories lists each Repository on a line of its own, in the order `settings.toml` gives: its number of
  Filters, "borrows from a clone" when `machine.toml` says its mirror borrows objects, and the size of its
  mirror on the disk, or "no mirror yet" before the first clone ends, or "can't measure the mirror" when
  the mirror, or a folder in it, can't be read. The core measures the mirrors each time the page shows,
  once the dialog has read the files, on a thread apart from the window's. A line opens the Repository's
  page. Add repository… under the list opens the add dialog over Settings, and Esc closes only that
  dialog. It waits for the files, and is off while Apply saves, as the other controls are. A Repository it
  adds is saved at once and opened, as from the Home page, and lists on the page and in the left column;
  Cancel keeps it. Switch to it, on a Repository Tsugiki has already, closes Settings as Cancel does,
  putting back the changes not applied, and switches to that Repository; while Apply saves, Settings stays
  open and the window stays where it is.
- A Repository's page opens with a header: its `owner/name`, then Read-only or Can write, its open Issues
  and PRs, and its last sync, as "Can write · 1,204 open Issues · 342 open PRs · Synced 2 min ago". Read-only
  follows the Repository's state, for any reason it holds, says each reason as the Issue and PR views do
  ("Read-only: your token can't write to python/cpython"), and changes as the mark does. The counts are
  GitHub's as a sync last kept them ([sync.md](sync.md)), so a large Repository counts all it has open, not
  the window Tsugiki holds. They change as soon as a sync saves new ones, a first sync included, and before
  a sync has counted them they don't show. The last sync says "Not synced yet" before the first one
  finishes, moves on each minute, and changes when a sync of that Repository finishes, open or not. Each
  part waits for the core's answer, and the line shows once one has come. The page's groups follow the
  header in this order: Conventions, First sync, Filters, Muted authors, On this machine, then Reset cache
  and Remove repository. Only Filters is built so far.
- A Repository's Filters group lists each Filter on a line of its own, in the order `settings.toml` gives
  and the Sidebar shows: its name and its search, each edited in place, ↑ and ↓, off at either end, and
  Delete. Add filter adds a Filter with a blank name and search at the end and puts the focus in its name. A
  Filter with a blank name or search turns its line red, and Apply stays off until it has both. The lines
  wait for the files and are off while Apply saves. Apply writes the Repository's `filters` in the form the
  file uses: blocks of their own, or the inline list the user wrote, keeping the user's comments. A Filter
  the page read keeps its table, with each key and comment on it, `on_workbench` among them, through a new
  name, a new search or a move, and its name and search change in place. A Filter Add filter added is
  written with its name and search alone, so it is off the Workbench. A Repository the file holds without
  `filters` gets the Filters it reads as written out first, as the switch writes them. No Filters left
  writes `filters = []`, since a Repository without the key reads as having the starters. A second Filter
  with the search of one before it, which Tsugiki never read, goes. Apply refuses two Filters with one
  search, naming it, and saves nothing. The Sidebar shows the Filters once Apply has saved them, as after a
  hand edit, and a new search loads its items as one added by hand does; Cancel leaves the file and the
  Sidebar as they were. The page has no "On the Workbench" switch yet (#1017).
- Keyboard lists every action with its key in force and where it works, and marks each key `[keys]` remaps
  ([keyboard.md](keyboard.md)).
- Advanced has a Language servers group with a row each for ty, clangd and rust-analyzer, then the idle
  time ([definitions.md](definitions.md)). A row has the path of the server's program with Browse…, which
  opens the system's file picker, the versions we tested and the version found, the server's state, and
  Open log. The state is Not set up, Ready, Running, Stopped (idle), or the server's last problem, for the
  open Repository, with the end of its error output when it stopped on its own. clangd has three problems
  of its own, such as missing system headers ([definitions.md](definitions.md), "When clangd can't
  answer"). Spaces around a path, and the quotes Explorer's "Copy as path" puts around it, go as it is
  typed. Open log opens the log the server writes for the open Repository, and is off with no Repository
  open. The paths live in `machine.toml` as `[language_servers.<name>] path`, and the idle time as
  `[language_servers] idle_minutes`, a whole number of minutes above 0 that takes effect at the servers'
  next idle check.
  Apply saves a changed path, checks it, and keeps the dialog open so its row can say what the check found;
  the button reads OK again. A `machine.toml` that doesn't parse refuses the Apply before either file is
  saved. The Definition peek's link to a server's row opens the dialog on Advanced with the focus on that
  row's path.
- `[appearance]` in `settings.toml` holds `theme`, the Theme the window shows or `"system"` to follow the
  system, and `light_theme` and `dark_theme`, which following the system shows
  ([themes.md](themes.md), [ADR 0021](../adr/0021-appearance-theme-names-a-theme-or-follows-the-system.md)).
  A `"light"` or `"dark"` from a file written before shows the Theme in `light_theme` or `dark_theme`. A pick
  for that side in the theme picker writes the name picked in its place, and an Apply keeps it, as only the
  `themes` folder can say which Theme the side's name gives. A value that isn't text keeps its
  key's default and shows in the status bar, and an Apply refuses until it is mended, as for `[diff]`.
  General's Appearance rows set all three keys, and the theme picker sets `theme`.
- `[diff] peek` in `settings.toml` says where the Definition peek opens: `"inline"`, the default, under the
  line of the word looked up, or `"split"`, below the diff or the file view's code, which stays where it
  was ([definitions.md](definitions.md)). A change applies to the next peek. A value Tsugiki can't read
  opens the peek inline and shows in the status bar, as a bad `[diff] files` does. Only a hand edit sets it
  for now: General gets the choice later (#370).
- `[keys]` in `settings.toml` remaps keys, one line per action ([keyboard.md](keyboard.md)). A line Tsugiki
  can't read keeps its action's default key and shows in the status bar, but doesn't refuse an Apply, which
  leaves `[keys]` as the user wrote it. Only a hand edit sets it for now: the Keyboard page shows the keys it
  gives, and gets remapping later (#552).
- Each `[[repositories.filters]]` in `settings.toml` takes `on_workbench`, which puts the Filter's items on
  the Workbench; a Filter without it is off ([workbench.md](workbench.md), #819). A Repository without
  `filters` reads as having the two starter Filters, both off. Of two Filters with the same `query`, Tsugiki
  reads the first. The switch in the Filter's view saves it at once, without Apply, on the first Filter with
  that search, and writes the Filters of a Repository the file holds without them first. A Repository's page
  gets the switch beside each of its Filters (#1017).
- `read_only = true` in a `[[repositories]]` table of `settings.toml` makes the Repository **Read-only** by
  the user's choice, whatever the token can do, and Tsugiki sends no write there
  ([token.md](token.md)). A Repository without it can write, and Tsugiki writes the key only when it is
  true. The choice travels with the file, while the token's mark stays in the database, and Tsugiki works
  out whether a write may go from both as it sends one. The Issue and PR views say "Read-only: you chose it
  in this Repository's settings" atop. Only a hand edit sets it for now: Add repository and the
  Repository's page get the switch (#915).
- Settings are kept in two TOML files, which Tsugiki reads when it starts and when the Settings dialog opens,
  so a hand edit takes effect at the next of those ([ADR
  0014](../adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)).
  A quit or a crash during a save leaves the file with its old text or its new, never cut short, and quitting
  waits a moment for a save under way. A save through a symbolic link changes the file it points to and keeps
  the link, and refuses, naming that file, while it isn't there. A save puts a new file in place, so a hard
  link stops sharing the file at the first save: link a file into a dotfiles repository with a symbolic link.
  `settings.toml` travels: settings, remapped keybindings, and each Repository with its Conventions,
  Filters and choice of Read-only; the user syncs it by pointing `settings_path` at a dotfiles repository or
  a synced folder.
  `machine.toml` stays: the token source, the editor command, paths, the clone each Repository borrows objects
  from, window places and zoom. A personal access token goes to the OS keychain. Pinned issues, Muted
  authors, Muted and Archived items, Local marks, Last seen times, and text typed into a composer but not yet
  sent are kept in the local database. Unsent text survives a reload, a crash or a refused write, and is
  cleared once it is sent ([ADR 0008](../adr/0008-view-models-and-invalidation-over-ipc.md)).
- A portable mode works as VS Code's: with a `data` folder next to `tsg.exe` (or `tsugiki-portable-data` next
  to Tsugiki.app), the files, the database and the mirrors all live there. Releases include a Windows portable
  `.zip`.
