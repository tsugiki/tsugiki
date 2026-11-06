# Changelog

Release notes for Tsugiki. Keep "WIP" current as changes land, and rename it to
`## Version X.Y.Z (YYYY-MM-DD)` in the PR that sets the version. The release workflow uses that section as the
release's body, and fails without it
([ADR 0012](docs/adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## WIP

## Version 0.1.1 (2026-11-07)

A new PR in the Repository you have open shows at its next refresh, even while CPython's first sync runs. The
database is the same as 0.1.0's, so 0.1.1 opens it as it is.

### Added

- Tsugiki writes a line to `tsugiki.log` each time the window misses a performance budget, such as a slow
  view query, a slow key or a long frame, and the start-up time of each launch that opens on a Workbench.
  Daily use then shows where Tsugiki is slow, without a recording from the performance HUD. A burst writes at
  most 10 lines of a kind a minute.

### Fixed

- Repositories take turns to sync, so a big first sync no longer holds back the others. The open Repository
  gets every other step. Before, a refresh waited behind a first sync of CPython, and a new PR took 12
  minutes to show. While another Repository syncs, the status bar names it beside the sync time.

### First install

Tsugiki isn't code-signed yet, so Windows and macOS warn you the first time you open it. To make sure a
download is the file Tsugiki's workflow built, follow
[Checking a download](https://github.com/tsugiki/tsugiki#checking-a-download) before you open it.

- **Windows:** unzip `Tsugiki_0.1.1_x64-portable.zip` anywhere and run `tsg.exe`. When Windows says "Windows
  protected your PC", click More info, then Run anyway. Tsugiki keeps its data in the `data` folder next to
  `tsg.exe`, so to keep 0.1.0's, move its `data` folder next to the new `tsg.exe`.
- **macOS:** open the `.dmg` and drag Tsugiki to Applications. macOS blocks it the first time, so open
  System Settings › Privacy & Security and click Open Anyway beside Tsugiki.

## Version 0.1.0 (2026-11-06)

The first version for daily work. It lists the Issues that wait on you in `python/cpython` and your own
Repositories, and you answer them on GitHub.

### Added

- Add a Repository by its name or its link. Tsugiki uses the GitHub CLI's token, so sign in with
  `gh auth login` first.
- A first sync loads what waits on you before anything else, so the Workbench is right in about a minute.
  Then it loads everything open in a small Repository, or the last 30 days of a large one like CPython. The
  status bar shows its progress, and a sync you quit resumes where it stopped. After that, Tsugiki refreshes
  on its own, and when you click the status bar.
- The Sidebar: the Workbench, Review requests, the starter Filters, and Filters you write in `settings.toml`,
  each with its count.
- The Workbench lists Issues by Needs attention, says why each one waits on you, and shows its Origin PRs with
  their Backports beneath them.
- The Issue view: the title, description and comments, and each PR with its state, CI and Reviews.
- Last seen and New activity: an Issue you've read stops looking unread until something new happens on it.
- `↑`/`↓` and `Enter` move through the list and open an Issue, and `Ctrl+Shift+G` opens it on GitHub. On
  macOS, `Cmd` replaces `Ctrl` in Tsugiki's keys.
- The window shows whether Tsugiki can reach GitHub, and who is signed in.
- `tsg` runs one Tsugiki and passes `tsugiki://` links and other arguments to it.
- Print, Reload and the other browser shortcuts stay off, so Tsugiki gets those keys.
- A portable `.zip` for Windows and a `.dmg` for Apple silicon Macs. An installer comes later, with Alerts.
- A performance HUD, on `Ctrl+Shift+H` or from the command palette. It shows the window's speed against
  its budgets, names what came before a long frame, and times opening an Issue, from the database and from
  GitHub. Hide it to write the recording's numbers to `tsugiki.log`.

### First install

Tsugiki isn't code-signed yet, so Windows and macOS warn you the first time you open it. To make sure a
download is the file Tsugiki's workflow built, follow
[Checking a download](https://github.com/tsugiki/tsugiki#checking-a-download) before you open it.

- **Windows:** unzip `Tsugiki_0.1.0_x64-portable.zip` anywhere and run `tsg.exe`. When Windows says "Windows
  protected your PC", click More info, then Run anyway. Tsugiki keeps its data in the `data` folder next to
  `tsg.exe`.
- **macOS:** open the `.dmg` and drag Tsugiki to Applications. macOS blocks it the first time, so open
  System Settings › Privacy & Security and click Open Anyway beside Tsugiki.
