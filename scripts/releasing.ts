// The decisions behind `just prepare-release` and `just release`, kept apart from the git, cargo and GitHub
// calls in prepare-release.ts and tag-release.ts so that tests can reach them. docs/releasing.md says when
// to run each.

import {
  notesFor,
  versionHeading,
  versionHeadingStart,
  WIP,
  workspaceVersion,
  workspaceVersionLine,
} from "./release-notes.ts";

/** The clone's branch, whether it has changes, and the commits of HEAD and `origin/main`. */
export interface Tree {
  branch: string;
  dirty: boolean;
  head: string;
  originMain: string;
}

/** Whether a tag exists in this clone, and on GitHub. */
export interface Existing {
  here: boolean;
  onGitHub: boolean;
}

/** Where CI's run on main's commit stands: a release PR that only sets the version runs no tests itself. */
export type MainCi = "passed" | "running" | "failed" | "missing";

/**
 * Reads what `gh run list --json status,conclusion` printed for CI's runs on main's commit, newest first.
 * Anything it can't read counts as `missing`, which stops the tag.
 */
export function mainCi(listed: string): MainCi {
  let runs: unknown;
  try {
    runs = JSON.parse(listed);
  } catch {
    return "missing";
  }
  const [run] = Array.isArray(runs) ? (runs as { status?: unknown; conclusion?: unknown }[]) : [];
  if (run === undefined) {
    return "missing";
  }
  if (run.status !== "completed") {
    return "running";
  }
  return run.conclusion === "success" ? "passed" : "failed";
}

/** `arg` when it's a version such as `0.2.4`, or `undefined` for anything else, such as `v0.2.4`. */
export function releaseVersion(arg: string | undefined): string | undefined {
  return arg !== undefined && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(arg) ? arg : undefined;
}

/** A version or milestone title as numbers, padded to three parts. Other titles sort last. */
function versionParts(title: string): number[] {
  const parts = title.split(".").map(Number);
  return parts.every(Number.isInteger) ? [...parts, 0, 0].slice(0, 3) : [Infinity, Infinity, Infinity];
}

/** Sorts versions or milestone titles, earliest first, for `Array.prototype.sort`. */
export function byVersion(a: string, b: string): number {
  const [x, y] = [versionParts(a), versionParts(b)];
  const at = x.findIndex((part, i) => part !== y[i]);
  return at === -1 ? 0 : Number(x[at]) - Number(y[at]);
}

/** The prefix of a release branch, the one kind the Release workflow rehearses (ADR 0012). */
const RELEASE_BRANCHES = "release/";

/** The branch of the PR that releases `version`. */
export function releaseBranch(version: string): string {
  return `${RELEASE_BRANCHES}${version}`;
}

/** Whether `branch` is a release branch, whatever version it names. */
export function isReleaseBranch(branch: string): boolean {
  return branch.startsWith(RELEASE_BRANCHES);
}

/** `cargoToml` with `version` in `[workspace.package]`, every other line as it was. */
export function withVersion(cargoToml: string, version: string): string {
  const lines = cargoToml.split("\n");
  lines[workspaceVersionLine(lines).at] = `version = "${version}"`;
  return lines.join("\n");
}

/**
 * `changelog` with its `## WIP` heading renamed to `version`'s, released on `date`, under a new empty
 * `## WIP`. The notes stay where they were, still to edit by hand.
 */
export function withReleasedWip(changelog: string, version: string, date: string): string {
  const lines = changelog.split("\n");
  const wip = lines.flatMap((line, at) => (line === WIP ? [at] : []));
  const [at] = wip;
  if (at === undefined) {
    throw new Error(`CHANGELOG.md has no section "${WIP}".`);
  }
  if (wip.length > 1) {
    throw new Error(`CHANGELOG.md has ${String(wip.length)} sections "${WIP}".`);
  }
  // Any heading for the version counts, even one whose date the release check wouldn't read.
  if (lines.some((line) => line.startsWith(versionHeadingStart(version)))) {
    throw new Error(`CHANGELOG.md has a section for ${version} already.`);
  }
  lines.splice(at, 1, WIP, "", versionHeading(version, date));
  return lines.join("\n");
}

/** `date` as `YYYY-MM-DD`, in the local time zone, as the maintainer's calendar shows it. */
export function isoDate(date: Date): string {
  const pad = (part: number): string => String(part).padStart(2, "0");
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** The reason `tree` can't start or tag a release, or `undefined` for a clean main that matches GitHub's. */
function treeRefusal(tree: Tree): string | undefined {
  if (tree.branch !== "main") {
    return `Switch to main first; this is ${tree.branch === "" ? "a detached HEAD" : tree.branch}.`;
  }
  if (tree.dirty) {
    return "Commit, delete or ignore every change first.";
  }
  if (tree.head !== tree.originMain) {
    return "main doesn't match origin/main: run `git pull --ff-only`, or push what main has that GitHub lacks.";
  }
  return undefined;
}

/** The reason `just prepare-release` won't start `version`, or `undefined` when it may. */
export function prepareRefusal(tree: Tree, version: string | undefined, current: string): string | undefined {
  if (version === undefined) {
    return "Usage: `just prepare-release X.Y.Z`, such as `just prepare-release 0.2.4`.";
  }
  if (byVersion(version, current) <= 0) {
    return `The version has to be later than ${current}, the one in Cargo.toml.`;
  }
  return treeRefusal(tree);
}

/**
 * The reason `just ship --release` won't push `branch`, or `undefined` when it may: the branch has to name
 * Cargo.toml's version, and CHANGELOG.md has to hold that version's notes.
 */
export function shipRefusal(branch: string, cargoToml: string, changelog: string): string | undefined {
  try {
    const version = workspaceVersion(cargoToml);
    const expected = releaseBranch(version);
    if (branch !== expected) {
      return `Cargo.toml says ${version}, so the branch should be ${expected}, not ${branch}.`;
    }
    notesFor(changelog, version);
  } catch (cause) {
    // Both throw only `Error`s, with a message for the maintainer.
    return `\`just release-check\` fails: ${(cause as Error).message}`;
  }
  return undefined;
}

/** Why `ci` stops a tag, or `undefined` once CI passed on main's commit. */
const CI_REFUSALS: Record<MainCi, string | undefined> = {
  passed: undefined,
  running: "CI is still running on main's commit: tag it once the run passes.",
  failed: "CI failed on main's commit: fix it in a PR, then tag the commit that passes.",
  missing: "CI hasn't run on main's commit yet, or gh can't say: wait for the run to pass, then tag.",
};

/** The reason `just release` won't push `tag`, or `undefined` when it may. */
export function tagRefusal(tree: Tree, tag: string, existing: Existing, ci: MainCi): string | undefined {
  const refusal = treeRefusal(tree);
  if (refusal !== undefined) {
    return refusal;
  }
  if (CI_REFUSALS[ci] !== undefined) {
    return CI_REFUSALS[ci];
  }
  if (existing.here) {
    return (
      `${tag} exists in this clone already. If its push failed, \`git push origin ${tag}\` sends it, ` +
      `and \`git tag -d ${tag}\` drops it.`
    );
  }
  if (existing.onGitHub) {
    return `${tag} exists on GitHub already: bump the version, or see docs/releasing.md to delete it.`;
  }
  return undefined;
}
