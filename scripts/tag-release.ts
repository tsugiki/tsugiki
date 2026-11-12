// `just release` tags a release once its PR has merged, which starts the release workflow: ask the user
// first. It takes the version from Cargo.toml, so the tag can't disagree with the downloads. It refuses
// unless the tree is clean, the branch is main and main matches origin/main after a fetch, and when the tag
// exists here or on GitHub, or `just release-check` fails. Then it tags HEAD, pushes the tag and prints the
// release run's link.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

import { notesFor, workspaceVersion } from "./release-notes.ts";
import { tagRefusal } from "./releasing.ts";

const WORKFLOW = "https://github.com/tsugiki/tsugiki/actions/workflows/release.yml";

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

/** Runs a command and returns its output, or `undefined` when it fails. */
function attempt(command: string, ...args: string[]): string | undefined {
  try {
    return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return undefined;
  }
}

const cargoToml = readFileSync("Cargo.toml", "utf8");
const changelog = readFileSync("CHANGELOG.md", "utf8");
let version: string;
try {
  version = workspaceVersion(cargoToml);
} catch (cause) {
  stop(cause instanceof Error ? cause.message : String(cause));
}
const tag = `v${version}`;
run("git fetch failed.", "git", "fetch", "-q", "origin", "main");
const refusal = tagRefusal(
  {
    branch: run("git can't name the current branch.", "git", "branch", "--show-current"),
    dirty: run("git status failed.", "git", "status", "--porcelain") !== "",
    head: run("git rev-parse failed.", "git", "rev-parse", "HEAD"),
    originMain: run("git rev-parse failed.", "git", "rev-parse", "origin/main"),
  },
  tag,
  {
    here: run("git tag failed.", "git", "tag", "--list", tag) !== "",
    onGitHub: run("git ls-remote failed.", "git", "ls-remote", "--tags", "origin", `refs/tags/${tag}`) !== "",
  },
);
if (refusal !== undefined) stop(refusal);
try {
  notesFor(changelog, version);
} catch (cause) {
  stop(`\`just release-check\` fails: ${cause instanceof Error ? cause.message : String(cause)}`);
}

run(`git can't make the tag ${tag}.`, "git", "tag", tag);
run(
  `git can't push ${tag}. It's still in this clone: push it with \`git push origin ${tag}\`.`,
  "git",
  "push",
  "-q",
  "origin",
  tag,
);
console.log(`Pushed ${tag}, which starts the release workflow.`);

// GitHub takes a few seconds to start the run, and names the tag as its branch. The commit tells this run
// from one an earlier push of the same tag started. The tag is out, so a failed lookup only loses the link.
const commit = run("git rev-parse failed.", "git", "rev-parse", "HEAD");
for (let tries = 0; tries < 10; tries += 1) {
  await sleep(3000);
  const url = attempt(
    "gh",
    "run",
    "list",
    "--workflow",
    "release.yml",
    "--branch",
    tag,
    "--commit",
    commit,
    "--limit",
    "1",
    "--json",
    "url",
    "--jq",
    ".[0].url // empty",
  );
  if (url !== undefined && url !== "") {
    console.log(url);
    process.exit(0);
  }
}
console.log(`The run hasn't shown up yet: look for it at ${WORKFLOW}`);
