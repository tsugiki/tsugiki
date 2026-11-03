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

`cargo codspeed build` builds on the release profile and adds `-Cstrip=none -Cdebuginfo=2`, and `cargo bench`
inherits the release profile too. So benchmarks use `codegen-units = 1`, as we ship, and CodSpeed keeps its
symbols. A change to `[profile.release]` shifts CodSpeed's numbers once, against a base built the old way, so
read that PR's report with this in mind.

## Disk

A clean test build needs 1.6 GB and `target/` reached 32 GB in about two days, so most of it is old builds.
Cargo keeps them: `deps` held 14 copies of the `bindings` test and 11 of `tsg`. `cargo clean` frees it all,
and the next test build costs about 75 seconds. `cargo-sweep` could remove only old builds, but we haven't
tried it here.

## What to try next, in order

1. **Tests inside the crates.** `AGENTS.md` has the rule. `core` doesn't depend on Tauri, so its tests don't
   link it. A file in `crates/app/tests/` that uses `tsugiki_app` adds a link against Tauri and WebView2, and
   today only `bindings.rs` does. Stage 2's diff renderer and highlighting will make that binary bigger.
2. **One test binary for `app`.** Past about 10 files that use `tsugiki_app`, make those modules of
   `tests/app/main.rs`, and filter with `cargo test --test app <name>`. Leave out files that don't use it, or
   they'll start linking Tauri. `stale-bindings` in `package.json` names `--test bindings`, so change it in
   the same PR.
3. **`rust-lld`.** Set `linker = "rust-lld"` for `x86_64-pc-windows-msvc` in `.cargo/config.toml` when link
   times start to bother someone. We haven't checked how it handles the Windows resources Tauri embeds.
4. **More crates.** This costs the most and gains the least, because a ticket that touches several crates
   rebuilds them anyway. Consider it only if builds stay slow after the steps above.
