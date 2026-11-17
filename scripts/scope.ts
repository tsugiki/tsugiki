// The CI jobs a change needs, from the paths it touches. CI's `changes` job and `just rust-gates` both ask
// this module, so the two agree on which paths count.

/**
 * What a CI job reads, which decides when it runs. `rust` is rustfmt, Clippy and the Rust tests with their
 * coverage gate, on Windows and macOS. `dependencies` is cargo-deny and cargo-machete. `ui` is ESLint,
 * Prettier, svelte-check, token-deps, Vitest and Playwright, on Windows and macOS.
 */
export type Job = "rust" | "dependencies" | "ui";

/** Every job, in the order CI lists them. A change to anything no rule below claims runs them all. */
export const EVERY_JOB: readonly Job[] = ["rust", "dependencies", "ui"];

/** Files outside `crates/` that a Rust check reads, so a change to one runs the Rust jobs. */
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

/** Files under `crates/` that a UI job reads, so a change to one runs the UI jobs too. */
const READ_BY_UI = new Set([
  // ui/e2e/tauri-csp.ts gives the Playwright pages the app's CSP.
  "crates/app/tauri.conf.json",
  // The ui job's token-deps check reads it (scripts/token-deps.ts).
  "crates/token/Cargo.toml",
]);

/** The Rust toolchain's files at the root of the repo. */
const RUST_ROOT = new Set(["Cargo.toml", "Cargo.lock", "clippy.toml", "deny.toml", "rust-toolchain.toml"]);

/**
 * Files that only the jobs that always run read, or no CI job at all: pre-commit's hooks and their typos
 * list, Codecov's settings, and the other workflows.
 */
const READ_BY_NO_JOB = new Set([
  ".pre-commit-config.yaml",
  "_typos.toml",
  "codecov.yml",
  ".github/dependabot.yml",
  ".github/workflows/advisories.yml",
  // The benchmarks' own rule reads it (`needsRustBenchmarks`).
  ".github/workflows/bench.yml",
  ".github/workflows/release.yml",
]);

/**
 * Docs, which only doc-width and pre-commit read. Markdown under `crates/` is Rust's: the highlighter's tests
 * read a sample there.
 */
function isDocs(path: string): boolean {
  return path.startsWith("docs/") || (path.endsWith(".md") && !path.startsWith("crates/"));
}

/**
 * The jobs a change to `path` needs. Docs need none, and a path no rule claims, such as a script, needs all.
 */
function jobsOf(path: string): readonly Job[] {
  const rust = READ_BY_RUST.has(path) || RUST_ROOT.has(path) || path.startsWith("crates/");
  const ui = READ_BY_UI.has(path) || (path.startsWith("ui/") && !isDocs(path));
  if (rust || ui) {
    return [...(rust ? (["rust", "dependencies"] as const) : []), ...(ui ? (["ui"] as const) : [])];
  }
  return isDocs(path) || READ_BY_NO_JOB.has(path) ? [] : EVERY_JOB;
}

/** The paths in what `git diff --name-only` printed, one a line. */
export function paths(listed: string): string[] {
  return listed.split(/\r?\n/).filter((path) => path !== "");
}

/** git's arguments for the diff of the two Cargo files between `revisions` that `isVersionBump` reads. */
export function cargoDiffArgs(...revisions: string[]): string[] {
  return ["diff", "-U0", "--no-color", "--no-ext-diff", ...revisions, "--", "Cargo.toml", "Cargo.lock"];
}

/** Runs git with these arguments and returns what it printed, or throws when git fails. */
export type Git = (...args: string[]) => string;

/** The paths a change touches, and the jobs it needs. */
export interface Changed {
  paths: string[];
  jobs: Job[];
}

/** The paths that changed between `revisions`, as `git diff` takes them. A rename counts as both. */
export function changedPaths(git: Git, revisions: readonly string[]): string[] {
  return paths(git("diff", "--no-renames", "--name-only", ...revisions));
}

/**
 * The paths that changed between `revisions`, as `changedPaths` gives them, and the jobs they need.
 * `untracked` lists files git would add, which count as changed too.
 */
export function changedJobs(
  git: Git,
  revisions: readonly string[],
  untracked: readonly string[] = [],
): Changed {
  const changed = [...changedPaths(git, revisions), ...untracked];
  return { paths: changed, jobs: jobs(changed, () => git(...cargoDiffArgs(...revisions))) };
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
 * The jobs a change to `paths` needs, in CI's order. With no path to read, nothing is ruled out. `cargoDiff`
 * gives what `git diff -U0` prints for `Cargo.toml` and `Cargo.lock`, which tells a release PR that only
 * sets the version from one that changes a dependency: such a PR needs only cargo-deny and cargo-machete,
 * which check that Cargo.lock followed. `cargoDiff` runs only when the paths could be such a PR.
 */
export function jobs(paths: readonly string[], cargoDiff: () => string): Job[] {
  if (paths.length === 0) {
    return [...EVERY_JOB];
  }
  if (onlySetsVersion(paths, cargoDiff)) {
    return ["dependencies"];
  }
  const needed = new Set(paths.flatMap(jobsOf));
  return EVERY_JOB.filter((job) => needed.has(job));
}

/** Whether `paths` are a release PR's, which only sets the version. `cargoDiff` runs only if they can be. */
function onlySetsVersion(paths: readonly string[], cargoDiff: () => string): boolean {
  // A new version moves each workspace crate's line in Cargo.lock, so a Cargo.toml change without one isn't
  // the workspace's version.
  const both = paths.includes("Cargo.toml") && paths.includes("Cargo.lock");
  return both && paths.every((path) => RELEASE_FILES.has(path)) && isVersionBump(cargoDiff());
}

/**
 * Files outside `crates/core/` that the Rust benchmarks read: the workspace's Cargo files and toolchain, and
 * the workflow that builds and runs them. tsugiki-core has no path dependency, and its benchmarks build their
 * database in code.
 */
const READ_BY_RUST_BENCHMARKS = new Set([
  "Cargo.toml",
  "Cargo.lock",
  "rust-toolchain.toml",
  ".github/workflows/bench.yml",
]);

/**
 * Whether a PR that changes `paths` against its base needs tsugiki-core's benchmarks. With no path to read,
 * nothing is ruled out. A release PR that only sets the version doesn't: it changes no code they measure.
 */
export function needsRustBenchmarks(paths: readonly string[], cargoDiff: () => string): boolean {
  if (paths.length === 0) {
    return true;
  }
  const read = paths.some((path) => path.startsWith("crates/core/") || READ_BY_RUST_BENCHMARKS.has(path));
  return read && !onlySetsVersion(paths, cargoDiff);
}

/** The commits a CI run on a PR tested: the PR's head, and the base it was merged with. */
export interface Tested {
  head: string;
  base: string;
}

const COMMIT = /^[0-9a-f]{40}$/;

/** Reads the `tested.json` a run left, or `undefined` for anything that isn't two commits. */
export function readTested(text: string): Tested | undefined {
  let tested: unknown;
  try {
    tested = JSON.parse(text);
  } catch {
    return undefined;
  }
  const { head, base } = (tested ?? {}) as { head?: unknown; base?: unknown };
  return typeof head === "string" && COMMIT.test(head) && typeof base === "string" && COMMIT.test(base)
    ? { head, base }
    : undefined;
}

/**
 * The jobs whose inputs changed between the tree a PR's last passed run tested and the checkout, with a line
 * for the log that says why. `jobs` is `undefined` when that tree can't be had, and then the whole PR counts:
 * `lastTested` is that run's `tested.json`, or `undefined` when no run passed or it left none. The tree is
 * the one git gets from merging that run's head into its base again, as the run did.
 */
export function sincePassed(lastTested: string | undefined, git: Git): { jobs?: Job[]; note: string } {
  if (lastTested === undefined) {
    return { note: "No run on this PR has passed yet, or it left no tested.json, so the whole PR counts." };
  }
  const tested = readTested(lastTested);
  if (tested === undefined) {
    return { note: "The last passed run's tested.json doesn't name two commits, so the whole PR counts." };
  }
  let tree: string;
  try {
    tree = git("merge-tree", "--write-tree", tested.base, tested.head);
  } catch {
    return { note: `git can't merge ${tested.head} into ${tested.base} again, so the whole PR counts.` };
  }
  let changed: Changed;
  try {
    changed = changedJobs(git, [tree, "HEAD"]);
  } catch {
    return { note: `git can't diff the tree ${tree} with the checkout, so the whole PR counts.` };
  }
  const testedAt = `The last passed run tested ${tested.head} on ${tested.base}, the tree ${tree}`;
  if (changed.paths.length === 0) {
    return { jobs: [], note: `${testedAt}, and nothing changed since.` };
  }
  return { jobs: changed.jobs, note: `${testedAt}. Since then: ${changed.paths.join(", ")}.` };
}

/**
 * The jobs a PR's run needs: those the whole PR needs against its base, less those whose inputs haven't
 * changed since a run that passed. `sincePassed` is the jobs the change since that run's tree needs, or
 * `undefined` when no such run can be found.
 */
export function narrowed(whole: readonly Job[], sincePassed: readonly Job[] | undefined): Job[] {
  return whole.filter((job) => sincePassed === undefined || sincePassed.includes(job));
}

/** The line CI's `changes` job writes to `$GITHUB_OUTPUT`: the jobs to run, as a JSON list. */
export function outputs(given: readonly Job[]): string {
  return `jobs=${JSON.stringify(given)}\n`;
}
