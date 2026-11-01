# Build order for v1

Tsugiki v1 is built in stages, each a thin slice through every layer ([ADR 0007](adr/0007-core-library-and-thin-tauri-app.md): `tsugiki-core`, the Tauri app, the Svelte UI). From stage 1 on, every stage ships as one or more 0.x releases that the maintainer uses every day ([ADR 0012](adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)). A stage may be split into as many releases as is useful; version numbers are given as releases go out, not here. 1.0.0 follows the last stage.

What v1 does is in [scope.md](scope.md), and the words are the glossary's ([CONTEXT.md](../CONTEXT.md)). The prototypes on `prototype/app-shell` and `prototype/diff-renderer` are the spec for behaviour, and their CPython snapshots are the first tests ([What carries over from the prototypes?](https://github.com/tsugiki/tsugiki/issues/8)). Every stage keeps the gates of [ADR 0009](adr/0009-testing-and-full-coverage.md) and [ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md) green.

## 0. Skeleton

Not for daily use. It proves the pipeline end to end before features land on it.

- The Cargo and npm workspaces, pinned toolchains, ESLint, Prettier and `svelte-check` ([ADR 0007](adr/0007-core-library-and-thin-tauri-app.md)).
- CI on Windows and macOS with both coverage gates; the CodSpeed job and the query-plan check on Linux ([ADR 0009](adr/0009-testing-and-full-coverage.md), [ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- The release workflow run on a draft: build, attest, `gh attestation verify`, without publishing ([ADR 0012](adr/0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md), [Verify release attestations in the release workflow before publishing](https://github.com/tsugiki/tsugiki/issues/6)).
- One path through every layer: the GraphQL transport with its replayer, the store with its first migration ([ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)), one view-model query with generated bindings and the `changed` event ([ADR 0008](adr/0008-view-models-and-invalidation-over-ipc.md)), and a Svelte view that renders it.
- `tsg` as a single instance, the WebView2 shortcuts off, the page's key guard ([ADR 0013](adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)), and `settings.toml` and `machine.toml` read and watched ([ADR 0014](adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md)).

## 1. Issues inbox

The first daily-usable release. It replaces the Issues part of the email inbox; every action still happens on GitHub.

- Add repository for `python/cpython`, with gh's token ([ADR 0004](adr/0004-auth-via-gh-cli-token.md), [ADR 0005](adr/0005-token-stays-in-the-rust-core.md)).
- The first sync, user's work first, and the tiered refresh ([ADR 0015](adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)); sync progress and problems in the status bar.
- **Conventions** and the link rules, with Backports under their Origin PRs and whole trees ([ADR 0011](adr/0011-pr-links-from-github-signals-by-repository-conventions.md)).
- The Sidebar: the Workbench, Review requests, the starter Filters, and Filters edited by hand in `settings.toml`, with counts.
- The Issue list ordered by **Needs attention**, with the previews in an **Alert**'s words ([ADR 0016](adr/0016-alerts-launch-tsugiki-links-own-toasts-on-windows-the-plugin-on-macos.md), the Workbench part).
- The Issue view: title, description and comments from `bodyHTML`, and its Origin PRs and Backports as rows with state, CI and review state.
- **Last seen** and **New activity**.
- The Issue and every PR row open on GitHub (`Ctrl+Shift+G`); `↑`/`↓`, `Enter` and Refresh.
- No mirror, no PR view, no writing.

## 2. Reading PRs

- The mirror: a bare clone borrowing from the user's clone, fetching changed PR heads ([ADR 0002](adr/0002-tsugiki-owns-its-git-state.md), [How is a Repository's mirror created and kept current?](https://github.com/tsugiki/tsugiki/issues/13), branch `research/mirror`).
- The PR view: the conversation and the diff renderer with row ranges in chunks, highlighting in Rust, expanding context, Generated files and NEWS entries ([ADR 0001](adr/0001-standalone-tauri-gui-with-in-app-review.md)).
- Threads in the diff, **Outdated threads**, and threads on lines outside the diff ([How are review threads on lines outside the diff shown?](https://github.com/tsugiki/tsugiki/issues/17)).
- **Stale reviews** by per-file patch-ids, and the Bot backport rules.
- Go to definition and **Definition peek**, the file view, the **Review checkout** and Open in editor ([ADR 0002](adr/0002-tsugiki-owns-its-git-state.md)).
- Read-only use offline.

## 3. Writing

- Comments on Issues and PRs; line and range comments into the **Pending review**; replies, resolve and unresolve, `suggestion` blocks; Submit review as Approve, Request changes or Comment.
- **Local marks**, and unsent composer text kept by the core ([ADR 0008](adr/0008-view-models-and-invalidation-over-ipc.md)).
- Refused writes, `write_refused`, and **Read-only** Repositories, with Copy and open on GitHub.
- A personal access token in the keychain, and the Account page with its guides ([ADR 0004](adr/0004-auth-via-gh-cli-token.md), [ADR 0005](adr/0005-token-stays-in-the-rust-core.md)).

## 4. Workbench breadth

- All issues, with search on GitHub and Load 30 more days ([ADR 0015](adr/0015-github-sync-serial-graphql-in-tiers-the-users-work-first.md)).
- **Pinned issues**, **Archive**, **Mute** and Mute author, and all the Built-in views.
- **Stacked PRs**, **Dependency updates**, **Maintained repositories**, the person Filter.
- The **Peek** for Issues and PRs outside the current Repository, and `tsugiki://` links.
- **Alerts** ([ADR 0016](adr/0016-alerts-launch-tsugiki-links-own-toasts-on-windows-the-plugin-on-macos.md)).

## 5. Settings and many Repositories

- The Settings dialog with all its pages, the Keyboard page's remapping, and the whole command palette.
- The Home page, several Repositories, and editing Conventions.
- Portable mode, Export and Import database ([ADR 0014](adr/0014-settings-in-toml-files-user-state-in-the-database-and-a-portable-mode.md), [ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)).
- Zoom and the macOS menu ([ADR 0013](adr/0013-webview2-browser-shortcuts-off-tsugiki-owns-its-keys.md)).

## 6. 1.0.0

- Every budget met in the real window on Windows and macOS ([ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- The release checklists run: the macOS checklist with its Alert checks, the `tauri-driver` smoke tests, the CPython mirror suite.
- The README with install notes for SmartScreen and Open Anyway, and how to check a download with `gh attestation verify`.
