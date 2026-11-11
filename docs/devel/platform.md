# Platform and stack

Part of [what v1 does](../scope.md). Terms in **bold** are defined in [CONTEXT.md](../../CONTEXT.md).

- A Tauri desktop app with a Svelte 5 + TypeScript frontend, for Windows and macOS ([ADR
  0001](../adr/0001-standalone-tauri-gui-with-in-app-review.md)).
- Diffs are drawn by a custom virtualized renderer, with syntax highlighting done in Rust.
- It stays fast on CPython: a usable Workbench 1 s after start-up, view queries within 50 ms, no frame over 25
  ms while scrolling any list or diff, a PR's conversation in 200 ms and its first diff rows in 1 s, and a
  background refresh that drops no frame. CodSpeed guards every PR against regressions, and the budgets are
  measured in the real window before each release ([ADR
  0017](../adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- Code comes from Tsugiki's own bare mirror. PRs, reviews, comments and CI come from the GitHub API ([ADR
  0002](../adr/0002-tsugiki-owns-its-git-state.md)).
- Git is driven through the `git` CLI, 2.30 or newer
  ([ADR 0003](../adr/0003-git-cli-over-libgit2-and-gitoxide.md)).
- `tsg` is the app's executable, and there is one instance of it ([ADR
  0007](../adr/0007-core-library-and-thin-tauri-app.md)). On its own, `tsg` starts Tsugiki or brings it to the
  front. `tsg python/cpython#12345`, an Issue or PR URL, or a `tsugiki://` link (what an **Alert** carries),
  opens that item in the running app, in its Repository if it has been added and in a **Peek** otherwise. The
  Windows installer, which installs for the current user without admin rights, puts `tsg` on the PATH; on
  macOS a command in the palette does it, as VS Code does for `code`.
- Releases are unsigned in v1: a per-user NSIS installer for Windows x64 and a universal `.dmg` for macOS,
  built from a tag, attested, and checked with `gh attestation verify` before they are published. Until Stage
  4, Windows gets only the portable `.zip` and the `.dmg` runs only on Apple silicon. Tsugiki doesn't update
  itself. It checks GitHub Releases at start-up and every 6 hours, the status bar says when a new version is
  out, and a click opens its release page; a Settings switch turns the check off. Versions follow SemVer, 0.x
  until v1, with notes kept in `CHANGELOG.md` ([ADR
  0012](../adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).
