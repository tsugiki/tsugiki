<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/logo/tsugiki-dark.svg">
    <img alt="tsugi木" src="docs/logo/tsugiki-light.svg" width="273" height="96">
  </picture>
</p>

<p align="center">A review workbench for CPython core developers, and for the projects they maintain.</p>

<p align="center">
  <a href="https://codecov.io/gh/tsugiki/tsugiki"><img alt="codecov" src="https://codecov.io/gh/tsugiki/tsugiki/graph/badge.svg?token=4DS69EB5ML"></a>
  <a href="https://app.codspeed.io/tsugiki/tsugiki?utm_source=badge"><img alt="CodSpeed" src="https://img.shields.io/endpoint?url=https://app.codspeed.io/badge.json"></a>
</p>

---

Tsugiki (接ぎ木, *grafting*) is a desktop app for taking in your GitHub work without the email inbox or the slow web UI. The name comes from **Backports**: a fix lands on main and is grafted onto each maintenance branch.

For the **Repositories** you add, such as `python/cpython` and your own projects, Tsugiki:

- groups PRs under the Issues they fix, with each fix's Backports beside it;
- works out what is waiting on you from GitHub's own data, not from notifications;
- lets you review inside the app, with a fast diff, review state kept per PR, and VS Code keybindings;
- keeps your GitHub token in its Rust core, away from the webview, and works with a read-only token if you'd rather not hand it write access.

## Status

Tsugiki is being designed. There is no app to install yet. The decisions made so far are written down here:

- [CONTEXT.md](CONTEXT.md): the glossary. Terms in **bold** are defined there.
- [docs/scope.md](docs/scope.md): what v1 does, and what it leaves out.
- [docs/adr](docs/adr/): architecture decisions and why they were made.
- [Issues](https://github.com/tsugiki/tsugiki/issues): open points to check before building.

It will be a [Tauri](https://tauri.app/) app (Rust with Svelte 5 and TypeScript) for Windows and macOS. Its command-line name is `tsg`.

## Reporting a bug

Tsugiki keeps a log of why things failed, and attaching it to a bug report helps. It stays on your computer
unless you send it, and Tsugiki hides your GitHub token in it. It can still name your paths, your login and
the links Tsugiki was asked to open, and quote what GitHub sent, so read it before you post it.

| Where Tsugiki runs | The log |
|---|---|
| Installed on Windows | `%LOCALAPPDATA%\tsugiki\tsugiki.log` |
| Installed on macOS | `~/Library/Application Support/tsugiki/tsugiki.log` |
| Portable on Windows | `data\tsugiki.log`, next to `tsg.exe` |
| Portable on macOS | `tsugiki-portable-data/tsugiki.log`, next to `Tsugiki.app` |

`tsugiki.log.1`, next to it, holds the older lines. A development build's folder is `tsugiki-dev`, not
`tsugiki`.

## License

[MIT](LICENSE). The wordmark is outlined from [Zen Kaku Gothic New](https://github.com/googlefonts/zen-kakugothic), under the SIL Open Font License 1.1.
