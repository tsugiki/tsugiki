// The checks a change needs, from the paths it touches. CI's `changes` job and `just rust-gates` both ask
// this module, so the two agree on which paths count.

/** The part of the repo a change touches: only docs, only the window's code and docs, or anything else. */
export type Scope = "docs" | "ui" | "all";

/** Files outside `crates/` that a Rust check reads, so a change to one runs the Rust checks. */
const READ_BY_RUST = new Set([
  // Drawn from the migrations, and crates/core/src/store/schema/tests.rs compares the two.
  "docs/schema.md",
  // Generated from the Rust commands, and the stale-bindings check compares the two.
  "ui/src/lib/data/bindings.ts",
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

/** The scope of a change to `paths`. With no path to read, nothing is ruled out. */
export function scope(paths: readonly string[]): Scope {
  if (paths.length === 0) {
    return "all";
  }
  if (paths.every(isDocs)) {
    return "docs";
  }
  return paths.every(isUi) ? "ui" : "all";
}

/** The lines CI's `changes` job writes to `$GITHUB_OUTPUT`. Docs alone count as `ui-only` too. */
export function outputs(given: Scope): string {
  return `docs-only=${String(given === "docs")}\nui-only=${String(given !== "all")}\n`;
}
