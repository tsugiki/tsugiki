# Testing: replayed GitHub, built git repos, and full coverage

Tests are built in layers. Fast ones run on every PR on Windows and macOS. The slow, live ones run before a release. Coverage is held at 100%, and CI fails below it.

- **GitHub.** The GitHub client talks through a small transport trait, and tests swap in a replayer that answers each GraphQL query from recorded JSON. A `record` mode of the same client captures real responses from public repositories (python/cpython, read-only) and scrubs them. Rare cases (a force-push after a review, a refused write, a 502 in the middle of a sync, the rate limit) are fixtures edited by hand or made with small builders. The tests run the real parsing and paging code.
- **git.** Tests build tiny repositories in a temp directory with the real `git` CLI, each set up for one case: a rebase that leaves the diff the same, a whitespace-only change, a merge from main, a force-push, a Generated file. A separate opt-in suite runs against a real CPython mirror (bootstrap, polling many PR heads, maintenance) before releases only.
- **Domain rules and view models.** Each rule has small scenario tests written in the glossary's words ("a Review request on a Draft PR needs attention"). View models ([ADR 0008](0008-view-models-and-invalidation-over-ipc.md)) are snapshot-tested with `insta` against recorded python/cpython data, starting from the app-shell prototype's snapshots. A change in what a screen shows then appears as a snapshot diff to review.
- **Frontend.** Vitest with Testing Library covers components and state, with the generated bindings mocked. Playwright drives whole flows against the built UI, with Tauri's IPC mocked, in Chromium and in WebKit (close to WKWebView): keys and chords, the palette, pane resizing, the Settings dialog. A few smoke tests of the real window run on Windows through `tauri-driver`: start, open a PR, Ctrl+P never prints. `tauri-driver` can't drive WKWebView, so macOS gets a short manual checklist before each release.

## Coverage

- Rust: 100% of lines and functions with `cargo-llvm-cov` on stable Rust, checked on Windows and on macOS
  separately. Code behind `#[cfg(windows)]` isn't in macOS's build, so it only counts on Windows. Code built
  on both has to be reached on both, or be excluded. Branch coverage would need nightly, and
  [ADR 0007](0007-core-library-and-thin-tauri-app.md) pins stable.
- TypeScript: 100% of lines, branches, functions and statements, measured by Vitest.
- Excluded, in one config per language, each with a reason: the generated bindings, `crates/app`'s Tauri
  start-up code and `crates/webview-keys`, which the smoke tests cover instead, and the file holding the
  core's real network and real `gh`, which tests replace and the release checks exercise. Any other exclusion
  carries a `// coverage: <reason>` comment that review sees.
- Codecov shows the same reports on each PR, with the uncovered lines. It only reports: its statuses are informational and a failed upload doesn't fail CI, so an outage there can't block a merge. The gates above decide.

## Considered Options

- **A trait-level fake for GitHub.** Easy to write, but parsing, paging, retries and the quirks of advanced search would go untested. **A local fake HTTP server** adds little beyond the replayer.
- **Checked-in fixture repositories.** Quicker to set up per test, but opaque binary blobs that are hard to change.
- **Running the CPython and live-GitHub suites on every PR.** They would catch drift sooner, but every PR would clone about 800 MB and spend the user's rate limit.
- **Linux-only PR checks.** Fast, but Linux isn't a v1 target, and WebView2 or WKWebView breakage would surface only at release.
- **Merging the Windows and macOS reports before the check.** Code built on both OSes but reached on one would
  pass. `cargo-llvm-cov` can't merge reports from two machines, so we had our own script, which matched
  functions by mangled name. It failed a PR over code that PR's tests did reach, because a crate hash's
  length changes the rest of the name.

## Consequences

- On every push to main and every PR that changes more than docs, on Windows and macOS: rustfmt, clippy
  with warnings as errors, `cargo test`, the stale-bindings check, ESLint, Prettier, `svelte-check`,
  Vitest, Playwright, and both coverage gates. CI skips them on a PR that changes only Markdown files or
  `docs/`, and still reports `CI passed`.
- On those same pushes and PRs, on Linux: `cargo deny` and `cargo machete`. They build nothing, so one OS is
  enough. A weekly run also checks for new advisories, so one still reaches us in a week with no PRs.
- On every PR, on Linux: the CodSpeed benchmarks
  ([ADR 0017](0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)). The
  query-plan check from that ADR is a test in the core, so `cargo test` runs it on Windows and macOS.
- Before a release (in the release workflow, and runnable by hand): the CPython mirror suite, the `tauri-driver` smoke tests on Windows, a check that live GitHub responses still match the recorded fixtures (read-only), the macOS checklist, and the performance budgets measured in the real window ([ADR 0017](0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- Code is written to be testable: the clock, the transport, the git runner and paths are passed in, not reached for.
