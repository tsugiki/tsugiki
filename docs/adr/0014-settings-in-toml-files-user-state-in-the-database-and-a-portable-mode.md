# Settings in TOML files, user state in the database, and a portable mode

What the user configures lives in two TOML files they can read and edit. What the user does while working
lives in `tsugiki.db` ([ADR 0010](0010-one-sqlite-database-and-per-repository-mirrors.md)).

- **`settings.toml`** travels between machines: settings, remapped keybindings, and each Repository with its
  **Conventions** ([ADR 0011](0011-pr-links-from-github-signals-by-repository-conventions.md)) and Filters.
  Only values that differ from the defaults are written.
- **`machine.toml`** stays on its machine: the token source, the editor command, paths, the clone each
  Repository borrows objects from, the Repository to reopen, window places, pane widths and zoom. It may set
  `settings_path`, which puts `settings.toml` anywhere, such as a dotfiles repository or a synced folder. That
  is how the user syncs it.
- **`tsugiki.db`** holds the user's state with the cache: Pinned issues, Muted authors, Muted and Archived
  items, Local marks, Last seen, and unsent composer text. These move with Export and Import database.
- **The keychain** holds a personal access token, which never travels ([ADR
  0005](0005-token-stays-in-the-rust-core.md)).

Tsugiki reads both files when it starts and when the Settings dialog opens, and doesn't watch them. The files
can be edited by hand, but an edit made while Tsugiki runs waits until one of those moments, so a restart is
the way to apply one before the dialog exists. The dialog's Apply saves and applies what changed in it, with
no re-read, and Cancel or Esc puts it back. A file that doesn't parse is left untouched, the status bar says
which line is wrong, and the last good values stay in force. Tsugiki's own writes go through `toml_edit`, so
the user's comments and order survive; a value back at its default is removed unless the user commented on
it. Each setting in the Settings dialog is labelled with where it is stored: settings file, this machine,
database or keychain.

## Where the files are

```
installed, Windows
  %LOCALAPPDATA%\Programs\Tsugiki\tsg.exe       the program
  %LOCALAPPDATA%\tsugiki\                       tsugiki.db, tsugiki.log, machine.toml, repos\, webview\
  %APPDATA%\tsugiki\settings.toml               roaming, unless settings_path says otherwise

installed, macOS
  /Applications/Tsugiki.app
  ~/Library/Application Support/tsugiki/        settings.toml, machine.toml, tsugiki.db, tsugiki.log, repos/

portable
  <folder>\tsg.exe
  <folder>\data\                                settings.toml, machine.toml, tsugiki.db, tsugiki.log, repos\,
                                                webview\
  (macOS: tsugiki-portable-data/ next to Tsugiki.app)

development build, unless portable
  the installed folders, named tsugiki-dev       (ADR 0016)
  <repo>\                                       tsugiki.log, tsugiki.log.1
```

A development build that isn't portable writes its log in the root of the repo it was built from, the folder
`just dev` runs in, so you can read or paste its lines without opening `AppData`. Its database and the rest
stay in the `tsugiki-dev` folders. The build takes the repo's path from the core crate's folder at compile
time, and logs beside its database when that checkout has moved or gone.

**Portable mode** works as in VS Code: when a `data` folder sits next to `tsg.exe`, or a
`tsugiki-portable-data` folder next to `Tsugiki.app`, everything lives there instead. The folder can be
carried to another machine, but it shouldn't sit in a syncing folder while Tsugiki runs, for the reason ADR
0010 gives. A personal access token stays in each machine's keychain. A portable copy and an installed one
count as the same program ([ADR 0007](0007-core-library-and-thin-tauri-app.md)), so starting one while the
other runs brings the running one forward.

## Considered Options

- **Everything in the database**, with no files: one thing to move, but settings could be changed only in the
  Settings dialog, and they couldn't be read, diffed or kept in a dotfiles repository.
- **JSONC files, as VS Code has them.** Familiar, but noisier to write by hand than TOML. **Watching both
  files and applying a hand edit at once.** Handy for someone tuning a setting in an editor, but it needs
  either a polling thread or the `notify` crate with its handling of editors that replace files and of a
  missing folder, for edits that are rare while Tsugiki runs. **Reading the files only at start-up**, so a
  hand edit waits for a restart even with the dialog open.
- **Pinned issues and Muted authors in `settings.toml`**, as scope.md first had them. They change as the user
  works, from the Sidebar and the Issue list, which is state, not configuration. Keeping them in the file
  would mean rewriting it all the time.
- **Fixed locations only**, synced by a symlink the user makes, or **all files next to the database**, synced
  by copying.
- **Data always beside the program**, with no app-data folders. On macOS nothing may be written inside
  `Tsugiki.app`, and on Windows the uninstaller would have to tell data from program files.
- **Keeping Tauri's per-user install folder**, `%LOCALAPPDATA%\tsugiki`, which is where ADR 0010 put the data.
  The program and its data would share a folder.

## Consequences

- The per-user NSIS installer, from Stage 4 ([ADR
  0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)), installs into
  `%LOCALAPPDATA%\Programs\Tsugiki`, as VS Code's user setup does. Its "Delete app data" box removes
  `%LOCALAPPDATA%\tsugiki` and `%APPDATA%\tsugiki`, not the `io.github.tsugiki` folder Tauri's uninstaller
  would remove.
- On Windows, `webview\` holds WebView2's profile and cache, so a portable copy leaves nothing behind. On
  macOS WKWebView picks its own folder, which Tauri can't move, so a portable copy there does leave that.
- Releases carry a Windows portable `.zip`, attested like everything else. It's all Windows gets until Stage
  4.
- A Repository removed from `settings.toml` by hand keeps its rows in the database, so adding it back restores
  its pins and marks. Remove repository in Settings deletes them and the mirror, as ADR 0010 says.
- A missing `settings.toml` at `settings_path` starts from the defaults, and the status bar says so. If it
  goes missing while Tsugiki runs, the last good values stay, since it may only be a drive gone for now.
