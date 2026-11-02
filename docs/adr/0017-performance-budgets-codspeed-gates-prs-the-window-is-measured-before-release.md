# Performance budgets: CodSpeed gates PRs, the window is measured before release

Tsugiki has to feel instant on CPython: about ten thousand open Issues and PRs, and diffs of up to 150k rows. [ADR 0008](0008-view-models-and-invalidation-over-ipc.md) runs a screen's whole query again on every `changed` event, which works only if those queries are fast. These are the budgets, measured on CPython-sized data:

| What | Budget |
|---|---|
| Start-up to a usable Workbench, from the database, without waiting on the network | 1 s |
| A view query (`workbench`, `issue`, `pr`, `sidebar`) in the core, p95 | 50 ms |
| Keys, selection, typing in a search box | the next frame |
| Palette results | 50 ms |
| Scrolling or flinging any list or diff | no frame over 25 ms |
| Opening a PR: the conversation / the first diff rows | 200 ms / 1 s |
| Jumping into a file not yet highlighted | 100 ms |
| A background refresh | no dropped frame |

The diff prototype ([ADR 0001](0001-standalone-tauri-gui-with-in-app-review.md)) already meets most of the diff's numbers. Jumping into a file it hadn't highlighted took up to 214 ms, so per-chunk highlighting is needed to reach 100 ms.

## How they are checked

- **On every PR**, a `bench.yml` job on `ubuntu-latest` runs the benchmarks under CodSpeed's simulation mode. It counts CPU work under Valgrind, so results don't depend on how busy the runner is. The benchmarks are `tsugiki-core`'s, written with divan (view queries, sync parsing, diff rows, highlighting, the link rules), and the TypeScript logic's, written with Vitest (the list model, the keymap, palette ranking). They run on a database scaled to CPython's size, built from the recorded fixtures ([ADR 0009](0009-testing-and-full-coverage.md)). A regression past CodSpeed's threshold fails a required check, unless it is acknowledged in CodSpeed. CodSpeed is free for open-source projects.
- **Also on every PR**, a test runs each view query's statements through SQLite's `EXPLAIN QUERY PLAN` and fails on a full scan of a large table, such as Issues, PRs or comments. This catches a missing index directly, even before the data is large enough for the benchmark to show it.
- **Before a release**, the millisecond budgets are measured in the real window on the maintainer's Windows and macOS machines, with the prototype's in-page harness (frame times, p95, worst frame, first paint) and timed core runs. They are compared with the budgets and with the last release. A miss blocks the release ([ADR 0012](0012-unsigned-v1-releases-from-a-tag-attested-and-checked.md)).

## Considered Options

- **Fewer, looser budgets** (2 s start-up, 100 ms queries, 50 ms frames), or **no numbers** until v1 is built. With looser numbers, re-querying on `changed` would show. Without numbers, nothing would stop slow code from landing.
- **Timed benchmarks on every PR**, with 3× slack for GitHub's shared runners. They would either flake or catch only gross regressions. **Checking by hand** before each release would catch regressions late, with no record.
- **CodSpeed reporting only**, without failing the PR. Regressions would rely on someone reading the report.
- **CodSpeed's walltime mode on its bare-metal runners**, checking the core against the millisecond budgets on each merge. It fits in the free 600 minutes a month, but simulation already catches regressions, and the budgets that matter most are the window's, which CodSpeed can't measure.

## Consequences

- The benchmark job is the one PR check that runs on Linux. That is safe because `tsugiki-core` doesn't depend on the OS, and [ADR 0009](0009-testing-and-full-coverage.md)'s reason for testing on Windows and macOS was the webviews, which this job doesn't touch.
- Benchmarks are code like any other: each core view model and hot path gets one, and the coverage rules apply to the helpers that build their data.
- A benchmark measures a realistic workload, not a single call: a `workbench` query on the CPython-sized database, or highlighting a whole 10k-row diff rather than one line. A very short benchmark is a poor signal even when it repeats exactly, because one extra allocation can pass the threshold and block a PR.
- The `tsugiki-core` crate must build on Linux, so platform-specific code stays in `crates/app` or behind `cfg` ([ADR 0007](0007-core-library-and-thin-tauri-app.md)).
