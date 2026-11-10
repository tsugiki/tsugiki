## Agent skills

### Issue tracker

GitHub Issues on tsugiki/tsugiki, through the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default roles, each label named after its role: `needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` and `docs/adr/` at the repo root, plus `docs/scope.md` and `docs/build-order.md`.
See `docs/agents/domain.md`. `docs/architecture.md` maps the crates, threads and modules: read it before
you search the code.

## Commands

Use [`just`](https://github.com/casey/just), and the `justfile` says how to install it. `just check` runs
every test CI runs, once each, one at a time on a few cores, because someone is working on this computer while
it runs. The Rust tests and the bindings check run inside `just coverage`, and `just cargo-test` and
`just stale-bindings` run them without coverage. `just` lists the rest.

Run `just check` once per PR, at the end, after `/code-review` and its fixes. Before that, run single test
files, `just typecheck`, `just doc-width` and the tests of each crate the change touches. If a gate fails, fix
it and rerun only that gate. Run `just check` again only if the fix changes behaviour outside that gate.

`just ship --check` does that last step: it runs `just check`, pushes and opens the PR, and prints only the
outcome, with the failed recipe and the log's tail on a failure. Plain `just ship` skips the check and pushes
a fix for a gate you reran yourself. `just next` lists the open tickets of the release being built.

A hook in `.claude/settings.json` refuses a shell command that rewrites code or docs in place, such as
`sed -i` or a redirect into `crates/`. Edit those files with the Edit or Write tool.

Put a `timeout` on a test run after you change what a parser requires: a session test can loop forever on a
reply it can't parse.

`just smoke` opens a window and presses real keys, so ask before each run. It runs before a release. In a
PR, run it only when the PR changes `crates/app/src/startup.rs`, `crates/webview-keys`, the Tauri config or
the Tauri version. Test everything else without a window: `wiring.rs` drives the commands and events on
Tauri's mock runtime.

Each push to a PR from a `release/` branch rehearses the release on CI, smoke test included, so ask before
opening one or pushing to it.

## Where tests go

Put a test inside its crate, in `core` where you can. Each file in a crate's `tests/` folder builds its own
binary, and one that uses `tsugiki_app` also links Tauri and WebView2. If more than about 10 files in
`crates/app/tests/` use it, move those into one `tests/app/main.rs` binary. `docs/build-costs.md` has the
numbers.

Each source file keeps its unit tests in a file of their own: `#[cfg(test)] mod tests;` in `sync/refresh.rs`
loads `sync/refresh/tests.rs`, and a `mod.rs` or `lib.rs` loads the `tests.rs` beside it. A test-only
`mod testing` of helpers lives in `testing.rs` the same way. You then read the code and its tests apart, and a
scan of the source can tell the two by path. `crates/app/tests/rules.rs` fails on an inline
`#[cfg(test)] mod name { ... }`.

## Rust design rules

Clippy enforces most of our rules, and `Cargo.toml` and `clippy.toml` give the reason for each. No tool can
check these six, so check them yourself when you write or review Rust:

- **Make invalid states unrepresentable.** Put each field on the enum variant it belongs to. Otherwise each
  reader has to remember which combinations can't happen, and the compiler can't help them.
- **Parse at the boundary.** Turn what `gh`, GitHub or the disk gives you into typed values where it enters
  the core. Code further in then has nothing left to re-check, and a bad answer fails in one place.
- **No trait with a single implementation.** A test double counts as a second. With only one, the trait adds a
  layer to read through and hides nothing.
- **No stringly typed data.** Give a value with a fixed set of choices an enum, and give values a caller could
  mix up their own types. The compiler then catches the typo or the swapped arguments, and a `String` catches
  neither.
- **One clear owner for each piece of data.** Everything else asks the owner or holds a read-only copy. With
  two owners the copies drift apart, and the UI shows whichever it read last.
- **One writer, many readers.** When threads share data, one thread writes it. The others read a copy through
  `latest::channel`, or send that thread a message asking for a change. Hold a lock only inside the type it
  guards, for one action, and never hand out a guard. Nobody can then take two locks in different orders, or
  hold one while waiting on GitHub or a disk. Prefer this in new code and when you refactor old code.

## Writing prose

Check docs, commit messages, issues and PRs with `/stop-slop` before you publish them. Run `just prose-check`
first: it lists lines worth rereading from what the branch adds, or from a draft you name, and never fails.

Wrap the docs you write at 110 columns: Markdown files, comments and docstrings. Leave tables unwrapped.
`just doc-width` fails on a line past that, and `just check` runs it.
