// `just ship` pushes the branch and opens the PR, or says which PR the push updated. The branch's first
// commit gives the PR its title and, without `just ship body.md`, its body.
// `just ship --check` runs `just check` first, which writes to .temp/check.log, and prints only its outcome
// and, on a failure, the failed recipe and the log's tail, so an agent reads a few lines of a long run.
// Ship a PR without it and let CI run the gates, unless it's a release PR or changes the gates (AGENTS.md).
// `just ship --release` is the only way it pushes a `release/` branch, since that push rehearses the release
// on CI: ask the user first. It refuses unless the branch names Cargo.toml's version and `just release-check`
// passes. `just ship --release --check` opens the release PR, and `just ship --release` pushes a fix to it.
import { execFileSync, spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";

import { branchRefusal, failureSummary, shipArgs } from "./pr.ts";
import { shipRefusal } from "./releasing.ts";

const LOG = ".temp/check.log";

function stop(message: string): never {
  console.error(message);
  process.exit(1);
}

/** Runs a command and returns its output, or stops with `failed`. */
function run(failed: string, command: string, ...args: string[]): string {
  try {
    return execFileSync(command, args, { encoding: "utf8" }).trim();
  } catch {
    stop(failed);
  }
}

function attempt(command: string, ...args: string[]): string | undefined {
  try {
    return execFileSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return undefined;
  }
}

const args = shipArgs(process.argv.slice(2));
if (args === undefined) stop("Usage: `just ship [--check] [--release] [body.md]`.");
const { check, release, bodyFile } = args;

const branch = run("git can't name the current branch.", "git", "branch", "--show-current");
const refusal = branchRefusal(branch, release);
if (refusal !== undefined) stop(refusal);
// Untracked files count too: the tests saw them, and the push would leave them out.
if (run("git status failed.", "git", "status", "--porcelain") !== "") {
  stop("Commit, delete or ignore every change first: the PR should hold what you tested.");
}
if (release) {
  const why = shipRefusal(branch, readFileSync("Cargo.toml", "utf8"), readFileSync("CHANGELOG.md", "utf8"));
  if (why !== undefined) stop(why);
}
mkdirSync(".temp", { recursive: true });

if (check) {
  const started = Date.now();
  const log = openSync(LOG, "w");
  const result = spawnSync("just", ["check"], { stdio: ["ignore", log, log] });
  closeSync(log);
  const minutes = ((Date.now() - started) / 60_000).toFixed(1);
  if (result.status !== 0) {
    console.error(failureSummary(readFileSync(LOG, "utf8")));
    stop(`\`just check\` failed after ${minutes} min; the whole log is in ${LOG}.`);
  }
  console.log(`\`just check\` passed in ${minutes} min.`);
}

run(
  "git push failed. If you rewrote the branch, ask the user before a force push.",
  "git",
  "push",
  "-q",
  "-u",
  "origin",
  branch,
);
const open = attempt(
  "gh",
  "pr",
  "view",
  branch,
  "--json",
  "url,state",
  "--jq",
  'select(.state == "OPEN") | .url',
);
if (open !== undefined && open !== "") {
  console.log(`Pushed to ${open}`);
} else {
  // The branch's first commit names the PR; later ones tend to be fixes from review.
  const first = run("git log failed.", "git", "rev-list", "--reverse", "origin/main..HEAD").split("\n")[0];
  if (first === undefined || first === "") stop("The branch holds no commit that main lacks.");
  const body = bodyFile ?? ".temp/ship-body.md";
  if (bodyFile === undefined) {
    writeFileSync(body, run("git log failed.", "git", "log", "-1", "--format=%b", first));
  }
  const title = run("git log failed.", "git", "log", "-1", "--format=%s", first);
  console.log(
    run(
      "gh pr create failed.",
      "gh",
      "pr",
      "create",
      "--base",
      "main",
      "--head",
      branch,
      "--title",
      title,
      "--body-file",
      body,
    ),
  );
}
