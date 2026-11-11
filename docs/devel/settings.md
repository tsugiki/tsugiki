# Settings

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- **Settings** is a dialog over the app, opened with `Ctrl+,`, ⚙ in the Sidebar or on the Home page, or the
  command palette. Its left column lists the pages (General, Account, Repositories, Sync, Keyboard, Advanced),
  with each Repository under Repositories as a page of its own. Repositories itself is an overview with Add
  repository. A Repository's page holds its conventions, first sync, the clone to borrow objects from, Filters
  and Muted authors; Pinned issues are not there, since the list can be long: they are managed from the
  Sidebar's Pinned issues (★ on an item, `Ctrl+K P`). Changes preview live; Apply keeps them, and Cancel or
  Esc puts them back. The dialog moves by its title bar, resizes from any edge or corner, and can be
  maximized; its size and place are kept per machine. Each setting is labelled with where it is stored
  (settings file, this machine, database, or keychain). The Account page is in [token.md](token.md). This was
  chosen out of three variants prototyped on branch `prototype/app-shell`: a searchable list like VS Code's,
  this dialog, and editing the config files as text.
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
