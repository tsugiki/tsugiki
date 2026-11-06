# Changelog

Release notes for Tsugiki. Keep "WIP" current as changes land, and rename it to
`## Version X.Y.Z (YYYY-MM-DD)` in the PR that sets the version. The release workflow uses that section as the
release's body, and fails without it
([ADR 0012](docs/adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## WIP

## Version 0.1.0 (2026-11-06)

The first release for daily use: an Issues inbox for `python/cpython` and your own Repositories. You read
here what waits on you, and act on GitHub.

### Added

- Add a Repository by its name or its link. Tsugiki uses the GitHub CLI's token, so sign in with
  `gh auth login` first.
- A first sync loads what waits on you before anything else, so the Workbench is right in about a minute.
  Then a small Repository loads everything open, and a large one like CPython the last 30 days. The status
  bar shows its progress, and a sync that stops picks up where it left off. After it, Tsugiki refreshes by
  itself, and on a click in the status bar.
- The Sidebar: the Workbench, Review requests, the starter Filters, and Filters you write in `settings.toml`,
  each with its count.
- The Workbench lists Issues by Needs attention, says why each one waits on you, and shows its Origin PRs with
  their Backports beneath them.
- The Issue view: the title, description and comments, and each PR with its state, CI and review state.
- Last seen and New activity: an Issue you've read stops looking unread until something new happens on it.
- `↑`/`↓` and `Enter` move through the list and open an Issue, and `Ctrl+Shift+G` opens it on GitHub. On
  macOS, `Cmd` replaces `Ctrl` in Tsugiki's keys.
- A window that follows whether GitHub can be reached and who is signed in.
- `tsg` runs one Tsugiki and passes `tsugiki://` links and other arguments to it.
- Tsugiki's keys reach Tsugiki: Print, Reload and the other browser shortcuts stay off.
- A portable `.zip` for Windows and a `.dmg` for Apple silicon Macs. An installer comes later, with Alerts.
- A performance HUD, on `Ctrl+Shift+H` or from the command palette, that shows the window's speed against
  its budgets and writes a recording's numbers to `tsugiki.log`. It says what came before a long frame, and
  times opening an Issue, from the database and from GitHub.

### First install

Tsugiki isn't code-signed yet, so Windows and macOS warn you the first time you open it. To check first
that a download is the file Tsugiki's workflow built, see
[Checking a download](https://github.com/tsugiki/tsugiki#checking-a-download).

- **Windows:** unzip `Tsugiki_0.1.0_x64-portable.zip` anywhere and run `tsg.exe`. When Windows says "Windows
  protected your PC", click More info, then Run anyway. Tsugiki keeps its data in the `data` folder next to
  `tsg.exe`.
- **macOS:** open the `.dmg` and drag Tsugiki to Applications. macOS refuses the first open, so open System
  Settings › Privacy & Security and click Open Anyway beside Tsugiki.
