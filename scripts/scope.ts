// The checks a change needs, from the paths it touches. CI's `changes` job and `just rust-gates` both ask
// this module, so the two agree on which paths count.

/**
 * The part of the repo a change touches: only docs, only the window's code and docs, only the version and
 * its notes, or anything else.
 */
export type Scope = "docs" | "ui" | "release" | "all";

/** Files outside `crates/` that a Rust check reads, so a change to one runs the Rust checks. */
const READ_BY_RUST = new Set([
  // Drawn from the migrations, and crates/core/src/store/schema/tests.rs compares the two.
  "docs/schema.md",
  // crates/core/src/keys/tests.rs checks each action's key against its table.
  "docs/devel/keyboard.md",
  // crates/core/src/theme/tests.rs checks each token's value against its table.
  "docs/devel/themes.md",
  // Generated from the Rust commands and the core's key table, and the stale-bindings check compares them.
  "ui/src/lib/data/bindings.ts",
  "ui/src/lib/data/keymap.ts",
  // crates/app/tests/security.rs fails on a Tauri plugin listed here.
  "ui/package.json",
  // crates/app/src/first_frame/tests.rs checks that the page starts on the window's background.
  "ui/src/app.html",
]);

/** Markdown under `crates/` doesn't count: the highlighter's tests read a sample there. */
function isDocs(path: string): boolean {
  if (READ_BY_RUST.has(path)) return false;
  return path.startsWith("docs/") || (path.endsWith(".md") && !path.startsWith("crates/"));
}

function isUi(path: string): boolean {
  return isDocs(path) || (path.startsWith("ui/") && !READ_BY_RUST.has(path));
}

/** The paths in what `git diff --name-only` printed, one a line. */
export function paths(listed: string): string[] {
  return listed.split(/\r?\n/).filter((path) => path !== "");
}

/** The files a release PR changes when it only sets the version (docs/releasing.md). */
const RELEASE_FILES = new Set(["CHANGELOG.md", "Cargo.toml", "Cargo.lock"]);

/**
 * A file's name in a diff's header, such as `--- a/Cargo.toml`, whatever prefix git's settings give it. No
 * line of Cargo.toml or Cargo.lock starts with `--` or `++`.
 */
const DIFF_HEADER = /^(?:---|\+\+\+)/;
const VERSION_LINE = /^version = "[^"]+"$/;

/**
 * Whether `diff`, as `git diff -U0` prints it for `Cargo.toml` and `Cargo.lock`, only moves the workspace
 * from one version to another: each line it takes out reads `version = "<old>"`, and each it puts in reads
 * `version = "<new>"`. A dependency's bump changes its `checksum` line too, and names other versions.
 */
export function isVersionBump(diff: string): boolean {
  let from: string | undefined;
  let to: string | undefined;
  let removed = 0;
  let added = 0;
  for (const line of diff.split(/\r?\n/)) {
    if (!(line.startsWith("+") || line.startsWith("-")) || DIFF_HEADER.test(line)) continue;
    const version = line.slice(1);
    if (!VERSION_LINE.test(version)) return false;
    if (line.startsWith("-")) {
      from ??= version;
      if (version !== from) return false;
      removed += 1;
    } else {
      to ??= version;
      if (version !== to) return false;
      added += 1;
    }
  }
  return removed > 0 && removed === added && from !== to;
}

/**
 * The scope of a change to `paths`. With no path to read, nothing is ruled out. `cargoDiff` gives what
 * `git diff -U0` prints for `Cargo.toml` and `Cargo.lock`, which tells a release PR that only sets the
 * version from one that changes a dependency. It runs only when the paths could be such a PR.
 */
export function scope(paths: readonly string[], cargoDiff: () => string): Scope {
  if (paths.length === 0) {
    return "all";
  }
  if (paths.every(isDocs)) {
    return "docs";
  }
  if (paths.every(isUi)) {
    return "ui";
  }
  // A new version moves each workspace crate's line in Cargo.lock, so a Cargo.toml change without one isn't
  // the workspace's version.
  const both = paths.includes("Cargo.toml") && paths.includes("Cargo.lock");
  const setsVersion = both && paths.every((path) => RELEASE_FILES.has(path));
  return setsVersion && isVersionBump(cargoDiff()) ? "release" : "all";
}

/**
 * The lines CI's `changes` job writes to `$GITHUB_OUTPUT`. Docs alone count as `ui-only` too. A `release`
 * change sets only `release-only`.
 */
export function outputs(given: Scope): string {
  const uiOnly = given === "docs" || given === "ui";
  return (
    `docs-only=${String(given === "docs")}\nui-only=${String(uiOnly)}\n` +
    `release-only=${String(given === "release")}\n`
  );
}
