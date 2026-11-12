// `just prepare-release X.Y.Z` starts the release PR of docs/releasing.md on a clean main that matches
// GitHub's. It makes the branch `release/X.Y.Z`, sets the version in Cargo.toml, runs `cargo check` so
// Cargo.lock follows, and names CHANGELOG.md's WIP notes after the version, under a new empty WIP. It commits
// nothing: the notes are still to write by hand, and `just ship --release` pushes the branch once they are.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

import { workspaceVersion } from "./release-notes.ts";
import { isoDate, prepareRefusal, releaseVersion, withReleasedWip, withVersion } from "./releasing.ts";

function stop(message: string): never {
  console.error(message);
  process.exit(1);
}

/** Runs a command and returns its output, or stops with `failed` after the command's own message. */
function run(failed: string, command: string, ...args: string[]): string {
  try {
    return execFileSync(command, args, { encoding: "utf8" }).trim();
  } catch {
    stop(failed);
  }
}

const cargoToml = readFileSync("Cargo.toml", "utf8");
const changelog = readFileSync("CHANGELOG.md", "utf8");
const version = releaseVersion(process.argv[2]);
let current: string;
try {
  current = workspaceVersion(cargoToml);
} catch (cause) {
  stop(cause instanceof Error ? cause.message : String(cause));
}
run("git fetch failed.", "git", "fetch", "-q", "origin", "main");
const refusal = prepareRefusal(
  {
    branch: run("git can't name the current branch.", "git", "branch", "--show-current"),
    dirty: run("git status failed.", "git", "status", "--porcelain") !== "",
    head: run("git rev-parse failed.", "git", "rev-parse", "HEAD"),
    originMain: run("git rev-parse failed.", "git", "rev-parse", "origin/main"),
  },
  version,
  current,
);
if (refusal !== undefined || version === undefined) stop(refusal ?? "No version.");

// Both texts are ready before anything changes, so a CHANGELOG.md it can't rewrite changes nothing.
let texts: [string, string];
try {
  texts = [withVersion(cargoToml, version), withReleasedWip(changelog, version, isoDate(new Date()))];
} catch (cause) {
  stop(cause instanceof Error ? cause.message : String(cause));
}
const branch = `release/${version}`;
run(`git can't make the branch ${branch}.`, "git", "switch", "-q", "-c", branch);
writeFileSync("Cargo.toml", texts[0]);
writeFileSync("CHANGELOG.md", texts[1]);
try {
  execFileSync("cargo", ["check", "-q"], { stdio: "inherit" });
} catch {
  stop(
    `\`cargo check\` failed on ${branch}, which holds the new version and CHANGELOG.md, so Cargo.lock may ` +
      "not follow yet. Fix the cause, then run `cargo check` on this branch.",
  );
}
console.log(
  `On ${branch}, with version ${version} in Cargo.toml and Cargo.lock.\n` +
    `Write the notes under "## Version ${version}" in CHANGELOG.md, and \`just release-check\` prints them.\n` +
    "Then commit, ask the user, and run `just ship --release --check`.",
);
