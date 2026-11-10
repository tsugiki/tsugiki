<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo/tsugiki-dark.svg">
    <img alt="tsugi木" src="docs/logo/tsugiki-light.svg" width="273" height="96">
  </picture>
</p>

<p align="center">A review workbench for CPython core developers, and for the projects they maintain.</p>

<p align="center">
  <a href="https://codecov.io/gh/tsugiki/tsugiki"><img alt="codecov"
    src="https://codecov.io/gh/tsugiki/tsugiki/graph/badge.svg?token=4DS69EB5ML"></a>
  <a href="https://app.codspeed.io/tsugiki/tsugiki?utm_source=badge"><img alt="CodSpeed"
    src="https://img.shields.io/endpoint?url=https://app.codspeed.io/badge.json"></a>
</p>

---

Tsugiki (接ぎ木, *grafting*) is a desktop app for taking in your GitHub work without the email inbox or the slow
web UI. The name comes from **Backports**: a fix lands on main and is grafted onto each maintenance branch.

For the **Repositories** you add, such as `python/cpython` and your own projects, Tsugiki:

- groups PRs under the Issues they fix, with each fix's Backports beside it;
- works out what waits on you from GitHub's own data, so a notification you missed hides nothing;
- lets you review inside the app, with a fast diff, review state kept per PR, and VS Code keybindings;
- keeps your GitHub token in its Rust core, away from the webview, and works with a read-only token if you'd
  rather not hand it write access.

## Status

Tsugiki 0.1 is the first version for daily work. It lists the Issues that wait on you, and you answer them
on GitHub. 0.2, on main and not released yet, reads PRs in the app. Writing to GitHub comes in a later stage
of [docs/build-order.md](docs/build-order.md). These files record the decisions behind it:

- [CONTEXT.md](CONTEXT.md): the glossary. Terms in **bold** are defined there.
- [docs/scope.md](docs/scope.md): what v1 does, and what it leaves out.
- [docs/adr](docs/adr/): architecture decisions, each with its reasons.
- [docs/architecture.md](docs/architecture.md): where the code is, and how its threads talk.
- [Issues](https://github.com/tsugiki/tsugiki/issues): open points and planned work.

It is a [Tauri](https://tauri.app/) app (Rust with Svelte 5 and TypeScript) for Windows and macOS. Its
command-line name is `tsg`.

## Features

### In 0.1

- Add a Repository by its name or its link. Tsugiki reads GitHub with the GitHub CLI's token.
- A first sync loads what waits on you before anything else, then the rest, and resumes where you quit it.
  After that Tsugiki refreshes on its own, and when you click the status bar. Repositories sync in turns, the
  open one first.
- The Sidebar: the Workbench, Review requests, the starter Filters and the Filters you write in
  `settings.toml`, each with its count.
- The Workbench lists Issues by Needs attention, says why each one waits on you, and shows its Origin PRs
  with their Backports beneath them.
- The Issue view: the description, the comments, and each PR with its state, CI and Reviews.
- An Issue you've read stops looking unread until something new happens on it.
- `↑`/`↓` and `Enter` move through the list, and `Ctrl+Shift+G` opens an Issue or PR on GitHub. The
  webview's browser shortcuts stay off.
- `tsg` runs one Tsugiki and hands it `tsugiki://` links.
- A performance HUD on `Ctrl+Shift+H`, and a line in the log for each performance budget the window misses.
- A portable `.zip` for Windows and a `.dmg` for Apple silicon Macs.

### In 0.2

- Each Repository gets a Mirror, a bare git clone, and each sync fetches the branches of the PRs your work
  reaches. A Mirror can borrow its objects from a clone you have, and the status bar says when git is
  missing or the disk is short.
- A PR opens in the Detail pane. Its Conversation tab shows the Reviews with their threads, and its Files tab
  shows the diff, read from the Mirror.
- The diff highlights C, Python, Rust, TypeScript, Svelte and 9 more languages, with code inside other code
  ([docs/highlighting.md](docs/highlighting.md)). Generated files start collapsed, the Files tab
  renders a PR's NEWS entries above the diff, and you can expand the context around each hunk.
- Unified or side-by-side diffs on `Ctrl+Alt+U`.
- Review threads in the diff, Outdated ones and those on lines outside the diff included.
- `F7` and `F8` move between changes and threads, `Ctrl+F` finds text in the diff, and `Alt+←` and `Alt+→`
  go back and forward.
- The edges between the panes drag, and their widths stay on the machine.
- Each Issue and PR shows GitHub's Octicon for its state, in GitHub's colors.

### In 0.2.1

- `Ctrl+B` hides the Sidebar, `Ctrl+Alt+B` the Issue list, and `Ctrl+K Z` both. "Reset Layout" in the
  command palette puts the panes back, and `machine.toml` keeps which of them show.
- A Repository picker on `Ctrl+R` and on the Repository's name in the Sidebar, which lists the Repositories
  in the order you last opened them. The window title names the open one.

### Planned

The [Issues](https://github.com/tsugiki/tsugiki/issues) hold the work in flight, and
[docs/build-order.md](docs/build-order.md) the stages after it:

- 0.2.2: the token's code in a crate of its own ([#273](https://github.com/tsugiki/tsugiki/issues/273)),
  and a word when a new version of Tsugiki is out ([#274](https://github.com/tsugiki/tsugiki/issues/274)).
- Reading PRs, continued: Stale reviews, Go to definition and Definition peek, the file view, the Review
  checkout, Open in editor, and reading offline.
- Writing: comments, line comments in a Pending review, replies, resolving threads, `suggestion` blocks and
  submitting a review; Local marks; a token of your own in the keychain.
- Workbench breadth: All issues, Pinned issues, Archive and Mute, the Built-in views, Stacked PRs, Dependency
  updates, the Peek, Alerts, a Windows installer and a `.dmg` for Intel Macs.
- Settings: the Settings dialog, remapping keys, the whole command palette, the Home page with several
  Repositories, editing Conventions, exporting the database, zoom and the macOS menu.
- 1.0: every performance budget met in the real window, on Windows and macOS.

## Installing

Tsugiki runs on 64-bit Windows and on Macs with Apple silicon. It reads GitHub with the
[GitHub CLI](https://cli.github.com/)'s token, so install `gh` and run `gh auth login` first. It reads diffs
with [git](https://git-scm.com/downloads) 2.30 or newer, which it finds on the `PATH`.

Download the latest release from [Releases](https://github.com/tsugiki/tsugiki/releases). Tsugiki isn't
code-signed yet, so Windows and macOS warn you the first time you open it:

- **Windows:** unzip the `x64-portable.zip` anywhere and run `tsg.exe`. When Windows says "Windows protected
  your PC", click More info, then Run anyway. Tsugiki keeps its data in the `data` folder next to `tsg.exe`.
- **macOS:** open the `.dmg` and drag Tsugiki to Applications. macOS blocks it the first time, so open
  System Settings › Privacy & Security and click Open Anyway beside Tsugiki.

Then add a Repository, such as `python/cpython`. Its first sync loads what waits on you before anything
else, and the status bar shows how far it has got.

## Checking a download

Tsugiki's downloads aren't code-signed, so Windows and macOS warn you when you open one. Each file's
attestation shows where it came from. Tsugiki's release workflow writes this public record as it builds the
file, and the record ties the file's exact bytes to the workflow and the commit it ran on. To check a file
you downloaded, use the [GitHub CLI](https://cli.github.com/):

```
gh attestation verify <file> --repo tsugiki/tsugiki --signer-workflow tsugiki/tsugiki/.github/workflows/release.yml
```

If this fails, the file isn't the one Tsugiki's workflow built, so don't run it, and open an issue. A pass
tells you where the file came from, and says nothing about bugs in the code.

## Reporting a bug

Tsugiki keeps a log of why things failed, so attach it to a bug report. It stays on your computer unless
you send it, and Tsugiki hides your GitHub token in it. It can still name your paths, your login and
the links Tsugiki was asked to open, and quote what GitHub sent, so read it before you post it.

| Where Tsugiki runs | The log |
|---|---|
| Installed on Windows | `%LOCALAPPDATA%\tsugiki\tsugiki.log` |
| Installed on macOS | `~/Library/Application Support/tsugiki/tsugiki.log` |
| Portable on Windows | `data\tsugiki.log`, next to `tsg.exe` |
| Portable on macOS | `tsugiki-portable-data/tsugiki.log`, next to `Tsugiki.app` |
| A development build, such as `just dev` | `tsugiki.log` in the repo's root |

`tsugiki.log.1`, next to it, holds the older lines.

## License

[MIT](LICENSE). The wordmark is outlined from [Zen Kaku Gothic
New](https://github.com/googlefonts/zen-kakugothic), under the SIL Open Font License 1.1. The icons of Issues
and PRs are GitHub's [Octicons](https://github.com/primer/octicons), under the MIT License, copyright GitHub,
Inc.; `ui/src/lib/ui/octicons.ts` holds them.
