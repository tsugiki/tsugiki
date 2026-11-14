# Build size, link time and disk use

What our builds cost, what we changed about it in #57, and what to try next. Measure before and after any
change here, with `cargo build --tests --timings` on an empty target folder. Point `CARGO_TARGET_DIR` at a new
folder under `.temp` so your `target/` stays as it is.

## Why this is written down

Another project of the maintainer's has 82 files in `tests/`, and each builds as its own binary linking the
whole app. A full test build wrote about 35 GB, and `target/` once filled the disk at 128 GB. Tsugiki has the
same setup at a smaller scale.

| On 2026-11-04, on Windows | That project | Tsugiki |
|---|---|---|
| Integration test files | 82 | 5 (465 lines) |
| Size of a test's PDB | about 400 MB | up to about 145 MB, before #57 |
| Size of a test binary | about 43 MB | about 10 MB |
| Our own code, without tests | 37k lines | about 5k lines |
| `target/` | 52 GB | 32 GB, of which `target/debug` is 25 GB |

## Debug builds

`[profile.dev]` keeps line tables for our crates and no debug info for dependencies. A panic's backtrace still
shows file and line, and a test in `crates/core/src/lib.rs` fails if it stops doing so. A debugger can step by
line, but it can't show local variables. To get them back for a session, set
`CARGO_PROFILE_DEV_DEBUG=true`.

A clean `cargo build --tests` on Windows, with `CARGO_BUILD_JOBS=4`:

| | Before | Only dependencies changed | After |
|---|---|---|---|
| Clean build | 114 s, 101 s | 81 s | 75 s, 76 s |
| Target folder after it | 3.2 GB | 2.2 GB | 1.6 GB |
| All PDBs | 652 MB | 222 MB | 169 MB |
| `tsg.pdb` | 167 MB | 51 MB | 29 MB |
| Rebuild after touching `core` | 7 s, 5 s | 4 s | 3 s, 3 s |

## Release builds

We rehearsed each set of settings once on GitHub's runners. Sizes are of the file a user downloads, in MB.

| Settings | Windows zip | macOS dmg | `tsg.exe` |
|---|---|---|---|
| Cargo's defaults | 4.78 | 5.04 | 13.48 |
| `strip` | 4.78 | 4.56 | |
| `codegen-units = 1`, `strip` | 4.54 | 4.21 | 12.44 |
| Those and `lto = "thin"` | 4.63 | 4.23 | |
| Those and `lto = true` | 4.59 | 4.09 | 12.33 |
| Those and `panic = "abort"` | 3.62 | 3.55 | 7.91 |

The rehearsals' build times can't compare settings: `npx tauri build` on macOS took 134 s to 330 s, and the
order didn't follow the settings. So we timed the Windows build locally, with `CARGO_BUILD_JOBS=4`:

| Settings | Build | Rebuild after touching `crates/app` | Rebuild after touching `core` | `tsg.exe` |
|---|---|---|---|---|
| Cargo's defaults | 154 s, 156 s | 2 s | 28 s | 13.41 MB |
| `codegen-units = 1`, `strip` | 171 s, 164 s | 1 s | 59 s | 12.38 MB |
| Those and `lto = true` | 196 s | 57 s | | 12.27 MB |

- **`codegen-units = 1` stays.** It saves 5% on Windows and 8% on macOS for a build about 10% longer. It also
  adds 30 seconds to `just smoke` after a change in `core`. If that starts to bother someone, the smoke test
  can build with `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16`, since it already builds apart from the downloads.
- **`strip` stays.** It saves 10% on macOS and nothing on Windows, where a release has no debug info to
  remove. A panic's message and place still reach the log. A native crash on macOS, in SQLite or in the
  `unsafe` WebView code, reports only addresses, and no release keeps a copy with symbols to read them with.
- **`lto` goes.** The zip grows 1% and the dmg shrinks 3%, and each rebuild relinks for about a minute. It may
  still make the code faster, but the one Rust benchmark so far times a few string copies, so we can't tell.
  Try it again when a realistic benchmark exists
  ([ADR 0017](adr/0017-performance-budgets-codspeed-gates-prs-the-window-is-measured-before-release.md)).
- **`panic = "abort"` goes.** On top of fat LTO, it saves 1 MB of zip, 0.5 MB of dmg and 4 MB of `tsg.exe`,
  but a panic in a background thread would close the window. `session::run` catches that panic to keep the
  window up and show "Tsugiki stopped checking GitHub". Tests unwind whatever the release profile says, so
  they wouldn't run the behaviour we ship, and `session.rs` refuses to build with the setting.

## Benchmarks

`cargo codspeed build` and `cargo bench` use the `bench` profile, which inherits `[profile.release]`, and
`cargo-codspeed` adds `-Cstrip=none -Cdebuginfo=2`. So benchmarks use `codegen-units = 1`, as we ship, and
CodSpeed keeps its symbols. CodSpeed reported no change at #57, against a base built with Cargo's defaults,
but its two benchmarks are small. A realistic one may shift when `[profile.release]` changes, so read that
PR's report with this in mind.

## Disk

A clean test build needs 1.6 GB and `target/` reached 32 GB in about two days, so most of it is old builds.
Cargo keeps them: `deps` held 14 copies of the `bindings` test and 11 of `tsg`. `cargo clean` frees it all,
and the next test build costs about 75 seconds. `cargo-sweep` could remove only old builds, but we haven't
tried it here.

## Time of `just check`

On 2026-11-08, after a change to `tsugiki-core`, with `CARGO_BUILD_JOBS=4` and `RUST_TEST_THREADS=2`,
`just check` took 232 seconds:

| Gate | Seconds |
|---|---|
| `rust-coverage` | 74.4 |
| `cargo-test` | 52.8 |
| `vitest` | 30.6 |
| `e2e` | 28.8 |
| `lint` | 23.3 |
| `typecheck` | 11.3 |
| `stale-bindings` | 4.8 |
| `deny`, `doc-width`, `machete`, `release-check` | 6.4 together |

`cargo-test` spent 8.5 seconds compiling and most of the rest running `tsugiki-core`'s 1,041 tests, and
`rust-coverage` ran the same tests again. The workspace has no doctests, so `cargo llvm-cov` runs every test
`cargo test` does, `bindings_are_up_to_date` among them. Since #252, `check` leaves out `cargo-test` and
`stale-bindings`. CI ran both as jobs of their own until 2026-11-14, so a failed job's title named the gate.
On main's last run with them, `cargo-test` took 4.2 minutes and `stale-bindings` 2.4 beside the 4.5 of
`rust-coverage`, two more Windows runners for no new test. `rust-coverage`'s log says whether a test failed
or coverage fell short.

`cargo llvm-cov` also cleans the workspace's build before each run, and recompiling took about 30 seconds.
Locally, `scripts/rust-coverage.ts` runs `cargo llvm-cov clean --profraw-only`, which deletes the last
run's line counts, and then passes `--no-clean`, which keeps the instrumented build. CI still cleans. With
cargo-llvm-cov 0.9.1 and a probe function in `crates/core/src/lib.rs`:

| Run | Source | Recipe | Gate | Seconds | Compiling |
|---|---|---|---|---|---|
| 1 | probe and a test that calls it | clean | pass | 69 | 26.0 |
| 2 | test removed, probe kept | `--no-clean` alone | fail on the probe's lines | 51 | 7.9 |
| 3 | same | profraw clean, `--no-clean` | fail on the probe's lines | 42 | 0.4 |
| 4 | probe removed | profraw clean, `--no-clean` | pass | 53 | 7.3 |
| 5 | same | clean | pass | 72 | 27.3 |

Runs 4 and 5 found the same 81 files, 17,692 lines and 4,753 functions, and the same of them unrun. Only hit
counts differed, in 9 files. cargo-llvm-cov 0.8.7 gave the same results.

A build under another toolchain, `Cargo.lock` or set of flags gets other file names, and stays in
`target/llvm-cov-target` beside the new one. cargo-llvm-cov reads every build there, so code removed since
then shows up as never run. A run with `CI=true` left such a build, for a reason we didn't find, and the next
run failed on lines the file no longer had. With its line counts gone, an old build can add false gaps but
can't hide a real one. So when `deps` holds two builds of one test target, the script cleans before the run,
and after a failed `--no-clean` run it runs again clean.

The workspace has no doctests. `cargo llvm-cov` runs them only on nightly, so a crate that gains one needs
`cargo-test` back in `check`.

After the change, with no Rust changed since the last coverage run, `just check` took 147 seconds:

| Gate | Seconds |
|---|---|
| `rust-coverage` | 44.4 |
| `vitest` | 35.3 |
| `e2e` | 27.7 |
| `typecheck` | 24.3 |
| `lint` | 11.0 |
| `deny`, `doc-width`, `machete`, `release-check` | 4.1 together |

A change to `tsugiki-core` adds about 8 seconds of compiling to `rust-coverage`, as in runs 2 and 4.

## Linker

On 2026-11-12 we timed `rust-lld` against the MSVC linker (#253), with Rust 1.99.1 and
`CARGO_BUILD_JOBS=4`. Each command ran after touching `crates/core/src/lib.rs` or `crates/app/src/lib.rs`,
so it rebuilt one crate and linked it again. Each linker had its own target folder under `.temp`, and
`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld` picked `rust-lld` without editing any file. After two
rounds to warm both folders, five rounds alternated the linkers. Medians, in seconds:

| Command, after touching the crate | MSVC | `rust-lld` | Saved |
|---|---|---|---|
| `cargo test -p tsugiki-core --no-run` | 4.25 | 4.12 | 3% |
| `cargo test -p tsugiki-app --no-run` | 5.32 | 4.67 | 12% |
| `cargo build -p tsugiki-app` | 3.04 | 2.85 | 6% |

The commands differ only in the linker, so the difference is what `rust-lld` saves: under a fifth everywhere,
and about half a second at most. So we kept the MSVC linker, and CI and the release job keep it too.
Switching would mean checking that `rust-lld` keeps the icon, version info and manifest Tauri embeds in
`tsg.exe`, and running a smoke test, for less than a second a build. Try again if link times grow, for
example once `crates/app/tests/` holds several binaries that link Tauri.

## What to try next, in order

1. **Tests inside the crates.** `AGENTS.md` has the rule. `core` doesn't depend on Tauri, so its tests don't
   link it. A file in `crates/app/tests/` that uses `tsugiki_app` adds a link against Tauri and WebView2, and
   today only `bindings.rs` does. Stage 2's diff renderer and highlighting will make that binary bigger.
2. **One test binary for `app`.** Past about 10 files that use `tsugiki_app`, make those modules of
   `tests/app/main.rs`, and filter with `cargo test --test app <name>`. Leave out files that don't use it, or
   they'll start linking Tauri. `stale-bindings` in `package.json` names `--test bindings`, so change it in
   the same PR.
3. **More crates.** This costs the most and gains the least, because a ticket that touches several crates
   rebuilds them anyway. Consider it only if builds stay slow after the steps above.
