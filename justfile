# Local commands for Tsugiki, with the same names as gcmon's where they mean the same. `just` lists them.
# Each one calls an npm script, and CI calls those scripts itself, so a check is the same in both places.
#
# Install it with `cargo install just --locked --version 1.59.0`. It stays out of package.json because
# `npm ci` also runs in the release's build job, and that job gets no more dependencies than it needs.
#
# `sh` is not on PATH in PowerShell on Windows, and `bash` there may be WSL, so recipes run through Windows
# PowerShell. Keep every recipe a single command with no shell-specific syntax, so it works under both shells.
set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

# Someone is working on this computer while the checks run, so they leave cores free.
export CARGO_BUILD_JOBS := env("CARGO_BUILD_JOBS", "4")
export RUST_TEST_THREADS := env("RUST_TEST_THREADS", "2")

# Show the available recipes
default:
    @just --list

# Install the npm dependencies
install:
    npm ci

# After GitHub merges a PR, switch to main, pull, and delete the PR's local branch, by default the current one
merged branch="":
    node scripts/merged.ts {{quote(branch)}}

# `just ship body.md` takes the PR's body from a file
# Push the branch and open its PR; `just ship --check` runs `just check` first, and `--release` ships a
# release/ branch: ask first
ship *args:
    node scripts/ship.ts {{args}}

# Start a release's PR on a clean, current main: make the branch, set the version, rename CHANGELOG's WIP
prepare-release version:
    node scripts/prepare-release.ts {{quote(version)}}

# Tag main with Cargo.toml's version once the release PR merged, and push the tag: ask first
release:
    node scripts/tag-release.ts

# List the open tickets of the earliest open milestone, each under its blockers, ready for an agent first
next:
    node scripts/next.ts

# Run the app with hot reload
dev:
    npm run dev

# Build what a release ships for this computer: `tsg.exe` on Windows, the dmg on macOS
build:
    npm run build

# Build for Windows and zip the portable copy
portable-zip: build
    node scripts/portable-zip.ts

# Format everything
fmt:
    npm run fmt

# Run every test CI runs, once each, one at a time. The coverage gate runs the Rust tests and checks the
# bindings, so `cargo-test` and `stale-bindings` stay out. It runs no doctests, and the workspace has none.
# `smoke` isn't one: it opens a window and presses real keys.
# Like CI, it skips the Rust gates on a branch that changes only `ui/` and docs
check: lint doc-width svelte-check vitest rust-gates release-check e2e

# Check the formatting, lint with ESLint, and run the pre-commit hooks on the workflows and the spelling
lint:
    npm run lint

# Fail on a line of Markdown, a comment or a docstring past 110 columns
doc-width:
    npm run doc-width

# List the prose this branch adds that's worth rereading, or that of the files you name. It never fails.
prose-check *files:
    node scripts/prose-check.ts {{files}}

# Type check with svelte-check, and lint Rust with Clippy
typecheck:
    npm run check

# Type check the UI with svelte-check
svelte-check:
    npm run svelte-check

# Lint Rust with Clippy
clippy:
    npm run clippy

# Run Clippy, `rust-coverage`, `deny` and `machete`, unless the branch changes only `ui/` and docs
rust-gates:
    node scripts/rust-gates.ts

# Run the Vitest and Rust tests, and check the generated bindings
test: vitest cargo-test stale-bindings

# Run the Vitest tests with their coverage gate
vitest:
    npm run vitest -- --maxWorkers=2

# Run the Rust tests, after fetching the ty and clangd they start
cargo-test:
    npm run cargo-test

# Fetch the ty and clangd that the Rust tests start into target/tools, unless they are there, and print the
# paths for TSUGIKI_TY, TSUGIKI_RUST_ANALYZER and TSUGIKI_CLANGD
test-servers:
    npm run test-servers

# Check that the generated bindings match the Rust code
stale-bindings:
    npm run stale-bindings

# Regenerate the bindings
bindings:
    npm run bindings

# Draw docs/schema.md afresh from the migrations, after a PR adds one
schema $TSUGIKI_UPDATE_SCHEMA="1":
    cargo test -p tsugiki-core --lib store::schema

# Write the snapshot of each sample in crates/core/src/highlight/samples afresh, for you to read in git's diff
highlight-samples $INSTA_UPDATE="always" $INSTA_FORCE_PASS="1":
    npm run highlight-samples

alias coverage := rust-coverage

# Run the Rust tests with their coverage gate, and check the generated bindings
rust-coverage:
    npm run rust-coverage

# Check the Rust dependencies with cargo-deny
deny:
    npm run deny

# Look for unused Rust dependencies
machete:
    npm run machete

# Print a tag's release notes, or those of the version in Cargo.toml, and fail if they're missing
release-check tag="":
    node scripts/release-check.ts {{tag}}

# Run the Playwright tests
e2e:
    npm run e2e -- --workers=2

# Open a real window and press real keys, which takes the foreground. `just smoke --at 3520,80` places it.
smoke *args:
    npm run smoke -- {{args}}

# Run the benchmarks
bench:
    npm run bench
