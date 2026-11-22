// `just ship` pushes the branch and opens the PR, or says which PR the push updated. The branch's first
// commit gives the PR its title and, without `just ship body.md`, its body.
// Before each push it runs `just quick`, the gates that take seconds, and `just ship --check` runs
// `just check` instead. Either writes to .temp/<recipe>.log, and ship prints only its outcome and, on a
// failure, the failed recipe and the log's tail, so an agent reads a few lines of a long run.
// Ship a PR without `--check` and let CI run the gates, unless it changes the gates (AGENTS.md).
// `just ship --release` is the only way it pushes a `release/` branch, since that push rehearses the release
// on CI: ask the user first. It refuses unless the branch names Cargo.toml's version and `just release-check`
// passes. A release PR that only sets the version ships with `just ship --release`, and CI runs the gates.
import { spawnSync } from "node:child_process";
import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";

import { branchRefusal, failureSummary, shipArgs } from "./pr.ts";
import { shipRefusal } from "./releasing.ts";
import { attempt, run, stop } from "./run.ts";

const args = shipArgs(process.argv.slice(2));
if (args === undefined) stop("Usage: `just ship [--check] [--release] [body.md]`.");
const { gates, release, bodyFile } = args;

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

const logPath = `.temp/${gates}.log`;
const started = Date.now();
const log = openSync(logPath, "w");
const result = spawnSync("just", [gates], { stdio: ["ignore", log, log] });
closeSync(log);
const minutes = ((Date.now() - started) / 60_000).toFixed(1);
if (result.status !== 0) {
  console.error(failureSummary(readFileSync(logPath, "utf8")));
  stop(`\`just ${gates}\` failed after ${minutes} min; the whole log is in ${logPath}. Fix it and commit.`);
}
console.log(`\`just ${gates}\` passed in ${minutes} min.`);

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
