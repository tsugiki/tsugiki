# Scripts

The repo's tooling, in TypeScript that Node runs as it is (`node scripts/ship.ts`), with no build step. Each
script starts with a comment that says what it does and how to call it. You call most of them through `just`
or `npm run`, and CI calls the same npm scripts.

## What each one does

| Script                | Called by                        | What it does                                                                    |
| --------------------- | -------------------------------- | ------------------------------------------------------------------------------- |
| `ship.ts`             | `just ship`                      | Pushes the branch and opens the PR; with `--check`, runs `just check` first     |
| `merged.ts`           | `just merged`                    | After GitHub merges a PR: switches to main, pulls, deletes the local branch     |
| `next.ts`             | `just next`                      | Lists the open tickets of the release being built, ready-for-agent first        |
| `pr.ts`               | `ship.ts`, `next.ts`             | Their arguments, branch rules, failure summary and milestone order              |
| `doc-width.ts`        | `just doc-width`                 | Fails on a line of Markdown, a comment or a docstring past 110 columns          |
| `doc-lines.ts`        | `doc-width.ts`, `prose-check.ts` | Picks out the lines of a file that hold prose                                   |
| `prose-check.ts`      | `just prose-check`               | Lists prose to reread against /stop-slop; never fails                           |
| `prose.ts`            | `prose-check.ts`                 | The rules behind `just prose-check`                                             |
| `rust-gates.ts`       | `just rust-gates`, `just check`  | Runs the Rust gates, or skips them on a branch that changes only `ui/` and docs |
| `changes.ts`          | CI                               | Tells CI's `changes` job whether a PR changes only docs, or only `ui/` and docs |
| `scope.ts`            | `rust-gates.ts`, `changes.ts`    | Which paths count as docs or as `ui/`, so CI and `just check` agree             |
| `rust-coverage.ts`    | `just rust-coverage`             | Rust's coverage gate: runs the tests and fails below 100%                       |
| `coverage-gaps.ts`    | `rust-coverage.ts`               | Finds the lines and functions the Rust coverage gate fell short on              |
| `token-deps.ts`       | `just lint`                      | Fails when `crates/token` gains a dependency off its list (ADR 0019)            |
| `token-crate.ts`      | `token-deps.ts`                  | That list, and how the check reads a Cargo manifest                             |
| `bindings.ts`         | `just bindings`                  | Regenerates the UI's TypeScript bindings from the Rust commands                 |
| `release-check.ts`    | `just release-check`, CI         | Checks a version against CHANGELOG.md and prints its release notes              |
| `release-notes.ts`    | `release-check.ts`               | The rules a version has to meet before a release builds                         |
| `portable-zip.ts`     | `just portable-zip`, release     | Zips the Windows portable copy, and with `--try` starts it                      |
| `smoke.ts`            | `just smoke`                     | Builds a release and drives its real window; ask before you run it              |
| `shell-edits-hook.ts` | `.claude/settings.json`          | The hook that refuses a shell command editing code or docs in place             |
| `shell-edits.ts`      | `shell-edits-hook.ts`            | The rules that hook applies                                                     |

## Adding one

`ui/tsconfig.json` type-checks these files, and `npm run lint` runs ESLint and Prettier on them. Vitest runs
the `*.test.ts` files here, under the same 100% coverage gate as the UI.

Put the logic in a module that a `*.test.ts` file tests, as `prose.ts` and `pr.ts` do. Keep the part
that talks to git, GitHub, cargo or a window in a thin entry script, and list that script under `exclude` in
`ui/vite.config.ts` with a `// coverage:` line saying why no test can run it. Then give it a recipe in the
`justfile` and add a row to the table above.
