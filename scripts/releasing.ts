// The decisions behind `just prepare-release` and `just release`, kept apart from the git, cargo and GitHub
// calls in prepare-release.ts and tag-release.ts so that tests can reach them. docs/releasing.md says when
// to run each.

import { notesFor, workspaceVersion } from "./release-notes.ts";

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

/** `arg` when it's a version such as `0.2.4`, or `undefined` for anything else, such as `v0.2.4`. */
export function releaseVersion(arg: string | undefined): string | undefined {
  return arg !== undefined && /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(arg) ? arg : undefined;
}

function isLater(version: string, than: string): boolean {
  const theirs = than.split(".").map(Number);
  for (const [at, part] of version.split(".").map(Number).entries()) {
    const other = theirs[at] ?? 0;
    if (part !== other) {
      return part > other;
    }
  }
  return false;
}

/** `cargoToml` with `version` in `[workspace.package]`, every other line as it was. */
export function withVersion(cargoToml: string, version: string): string {
  const lines = cargoToml.split("\n");
  const start = lines.indexOf("[workspace.package]");
  const end = lines.findIndex((line, at) => at > start && line.startsWith("["));
  const at = lines.findIndex(
    (line, i) => start !== -1 && i > start && (end === -1 || i < end) && /^version = ".+"$/.test(line),
  );
  if (at === -1) {
    throw new Error("Cargo.toml has no version in [workspace.package].");
  }
  lines[at] = `version = "${version}"`;
  return lines.join("\n");
}

/**
 * `changelog` with its `## WIP` heading renamed to `version`'s, released on `date`, under a new empty
 * `## WIP`. The notes stay where they were, still to edit by hand.
 */
export function withReleasedWip(changelog: string, version: string, date: string): string {
  const lines = changelog.split("\n");
  const wip = lines.flatMap((line, at) => (line === "## WIP" ? [at] : []));
  const [at] = wip;
  if (at === undefined) {
    throw new Error('CHANGELOG.md has no section "## WIP".');
  }
  if (wip.length > 1) {
    throw new Error(`CHANGELOG.md has ${String(wip.length)} sections "## WIP".`);
  }
  if (lines.some((line) => line.startsWith(`## Version ${version} (`))) {
    throw new Error(`CHANGELOG.md has a section for ${version} already.`);
  }
  lines.splice(at, 1, "## WIP", "", `## Version ${version} (${date})`);
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
  if (!isLater(version, current)) {
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
    if (branch !== `release/${version}`) {
      return `Cargo.toml says ${version}, so the branch should be release/${version}, not ${branch}.`;
    }
    notesFor(changelog, version);
  } catch (cause) {
    // Both throw only `Error`s, with a message for the maintainer.
    return `\`just release-check\` fails: ${(cause as Error).message}`;
  }
  return undefined;
}

/** The reason `just release` won't push `tag`, or `undefined` when it may. */
export function tagRefusal(tree: Tree, tag: string, existing: Existing): string | undefined {
  const refusal = treeRefusal(tree);
  if (refusal !== undefined) {
    return refusal;
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
