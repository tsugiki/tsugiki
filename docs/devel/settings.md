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
  The controls wait until then; if the files can't be read, they stay off and the dialog says why. Each
  change shows on the window at once: clearing Show the status bar hides the status bar behind the dialog.
  Apply saves the tables the dialog changed over what the file says then, so a hand edit to another table
  made while the dialog was open stays, and closes; with nothing changed the button reads OK and closes
  without saving. Cancel, Esc, ✕ or a press outside the dialog puts every change back. While Apply saves,
  Cancel and Esc wait for it, and a save the core refuses keeps the dialog open with the reason under the
  pages, where Cancel still puts the changes back.
- Each setting carries a label for where it is stored: settings file, this machine, database or keychain,
  with the file's name or "on this machine only" in its tooltip.
- General groups its settings; each group is a heading with its rows under it. Status bar has Show the
  status bar, and says that problems and being offline still show when it's off. Updates has Check for new
  versions (`[releases] check`), the one setting that still needs a restart, so it says "Takes effect at the
  next start" under its row. Both live in `settings.toml`. Repositories, each Repository's page, Sync and
  Keyboard say "Nothing to set here yet." for now.
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
  key's default and shows in the status bar, and an Apply refuses until it is mended, as for `[diff]`. For
  now only a hand edit sets `theme`, and the theme picker sets a side's key: General gets the three rows later
  (#757).
- `[diff] peek` in `settings.toml` says where the Definition peek opens: `"inline"`, the default, under the
  line of the word looked up, or `"split"`, below the diff or the file view's code, which stays where it
  was ([definitions.md](definitions.md)). A change applies to the next peek. A value Tsugiki can't read
  opens the peek inline and shows in the status bar, as a bad `[diff] files` does. Only a hand edit sets it
  for now: General gets the choice later (#370).
- `[keys]` in `settings.toml` remaps keys, one line per action ([keyboard.md](keyboard.md)). A line Tsugiki
  can't read keeps its action's default key and shows in the status bar, but doesn't refuse an Apply, which
  leaves `[keys]` as the user wrote it. Only a hand edit sets it for now: the Keyboard page gets it later
  (#551, #552).
- Settings are kept in two TOML files, which Tsugiki reads when it starts and when the Settings dialog opens,
  so a hand edit takes effect at the next of those ([ADR
  0014](../adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)).
  `settings.toml` travels: settings, remapped keybindings, and each Repository with its Conventions and
  Filters; the user syncs it by pointing `settings_path` at a dotfiles repository or a synced folder.
  `machine.toml` stays: the token source, the editor command, paths, the clone each Repository borrows objects
  from, window places and zoom. A personal access token goes to the OS keychain. Pinned issues, Muted
  authors, Muted and Archived items, Local marks, Last seen times, and text typed into a composer but not yet
  sent are kept in the local database. Unsent text survives a reload, a crash or a refused write, and is
  cleared once it is sent ([ADR 0008](../adr/0008-view-models-and-invalidation-over-ipc.md)).
- A portable mode works as VS Code's: with a `data` folder next to `tsg.exe` (or `tsugiki-portable-data` next
  to Tsugiki.app), the files, the database and the mirrors all live there. Releases include a Windows portable
  `.zip`.
