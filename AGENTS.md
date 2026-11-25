## Agent skills

### Issue tracker

GitHub Issues on tsugiki/tsugiki, through the `gh` CLI. See `docs/agents/issue-tracker.md`. Each product
ticket carries an `area:*` label for each file in `docs/devel/` it touches.

### Triage labels

The five default roles, each label named after its role: `needs-triage`, `needs-info`, `ready-for-agent`,
`ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: `CONTEXT.md` and `docs/adr/` at the repo root, plus `docs/scope.md` and `docs/build-order.md`.
`docs/scope.md` names what v1 does, and links each area to its file in `docs/devel/`: read only the files a
ticket touches. See `docs/agents/domain.md`. `docs/architecture.md` maps the crates, threads and modules:
read it before you search the code.

## Commands

Use [`just`](https://github.com/casey/just), and the `justfile` says how to install it. `just check` runs
every test CI runs, once each, one at a time on a few cores, because someone is working on this computer while
it runs. The Rust tests and the bindings check run inside `just coverage`, and `just cargo-test` and
`just stale-bindings` run them without coverage. `just` lists the rest.

While you work, run single test files with `just vitest-file <file>`, their paths relative to `ui/`, the
tests of each crate the change touches with `just test-crate <crate>`, `just typecheck` and `just doc-width`.
Then run `/code-review`, fix what it finds, and push with `just ship`, which runs `just quick`, pushes and
opens the PR. Push each later commit and each merge of main with `just ship` too. Leave `just check` and
`just coverage` to CI: it runs the gates `just check` runs, on the same scope, and macOS as well, and you
wait for its result before the merge. If CI fails, fix the gate that failed, rerun only that gate, and push
again.

Before a push, read the diff for what only macOS's CI catches: a `C:/` path isn't absolute there, and a
closure behind `cfg!(windows)` leaves a line macOS's coverage never runs. Once a branch is pushed, fix it with
new commits and bring main in with `git merge origin/main`. An amend or a rebase needs a force push, and each
force push needs the user's say. The PR squash-merges anyway.

Build each change that becomes a PR in a worktree of its own, and leave the main checkout on a clean main,
for reading, the tracker and research. Other sessions work on this computer at the same time, and when you
switch branches or commit in a checkout you share, you take their work along half-done.
Each PR starts from a ticket, so file one first for a change that has none. `just worktree <n>` makes the
worktree for ticket n under `.temp/`. Each worktree builds Rust from scratch into a `target/` of its own, so
delete it once its PR has merged: `just worktree-remove <n>` deletes the worktree, its builds and its branch.
Use it in place of `just merged`, which switches to main, as git refuses that while the main checkout holds
main.

Before you build a ticket, read the tickets that settle rules for the same code, open or closed and in any
milestone, the security milestone included, and grep the code and docs for comments that name a later
ticket. Where one disagrees with the ticket, follow the later decision, and raise the conflict with the user
before you build.

Put a follow-up ticket that a review or a build files in the milestone of the release being built, the one
`just next` lists, not in a later one.

To build several tickets of a milestone at once, one background agent per ticket, use the `wave` skill in
`.claude/skills/wave/`.

Run `just preflight`, the gates a first push most often fails on, before a first push where a CI round trip
costs more, such as a wave's.

Ship a PR that changes the gates themselves, the `justfile`, `scripts/` or `.github/workflows/`, with
`just ship --check`, which runs `just check` before the push. `just next` lists the open tickets of the
release being built. `just tree 393` draws the open tickets below #393 as a tree of blockers, with how deep
each chain runs.

Put a `timeout` on a test run after you change what a parser requires: a session test can loop forever on a
reply it can't parse.

`just smoke` opens a window and presses real keys, so ask before each run. It runs before a release. In a
PR, run it only when the PR changes `crates/app/src/startup.rs`, `crates/webview-keys`, the Tauri config or
the Tauri version. Test everything else without a window: `wiring.rs` drives the commands and events on
Tauri's mock runtime.

Ask before opening or pushing to a PR from a `release/` branch, since each push rehearses the release on CI,
smoke test included, and before `just release`, whose tag starts the release workflow. `docs/releasing.md`
has the steps.

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

Before you write a test helper, grep for one that does the job. A Rust helper lives in the `testing.rs` of the
module or crate it serves. When the tests of several crates need it, it goes in `crates/test-git` if it uses
no crate's types, and otherwise in a public `testing` module of the crate whose types it uses:
`tsugiki_core::testing`, `tsugiki_core::github::testing`, `tsugiki_core::releases::testing`,
`tsugiki_lsp::testing` or `tsugiki_token::testing`. Such a module compiles under
`cfg(any(test, feature = "testing"))`, and the crates that use it turn on `testing` in `[dev-dependencies]`,
so no release build has it. It can't use `crates/test-git`, which only tests may depend on, so what needs that
stays behind `cfg(test)` inside it. A helper that a second test file needs moves there.

The UI's shared test helpers live in `ui/src/lib/testing/`, which only tests may import. `setup.ts` fills
jsdom's gaps, such as `ResizeObserver`, for every test file. `fixtures.ts` builds what the core answers, such
as a Workbench page or a ready diff, and imports no Vitest, so the e2e specs use it too. `helpers.ts` drives
the DOM, as `keydown` and `nextTask` do, and `state.ts` holds the helpers that reach into the app's bindings
and state. What the e2e specs alone share lives in `ui/e2e/fixtures.ts`.

CodeQL reads tests too, on CI only, as no local gate runs it. A test's assert or panic message prints no login
or token message, such as an `Account` or a `Status`, nor any value from the same tuple or row: its taint
spans the whole tuple, and `rust/cleartext-logging` fails the PR. A test strips no tags or other markup with
`replace`, even from our own sources: `js/incomplete-multi-character-sanitization` fails it.

## One copy

Code that a second place needs moves to one place that both use, in Rust, TypeScript, Svelte and the
scripts alike. Two copies drift apart once someone edits one and misses the other. Keep two
copies only where they change for different reasons, and say why in the PR. Two files that can't share code,
such as a workflow and a script, get a test that fails when they disagree. A comment that names the other
copy goes stale.

`just dupes` fails when two files hold the same block of code, from about five lines up. A repeat that stays
goes in `ALLOWED` in `scripts/copies.ts`, with why. Repeats inside one file pass.

## Rust design rules

Clippy enforces most of our rules, and `Cargo.toml` and `clippy.toml` give the reason for each. No tool can
check these, so check them yourself when you write or review Rust:

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

Route each statement to the file that owns it, as `docs/agents/prose.md` says, then check docs, commit
messages, issues and PRs with `/stop-slop` before you publish them. Run `just prose-check` first: it lists
lines worth rereading from what the branch adds, or from a draft you name, and never fails.

Wrap the docs you write at 110 columns: Markdown files, comments and docstrings. Leave tables unwrapped.
`just doc-width` fails on a line past that, and `just check` runs it.
