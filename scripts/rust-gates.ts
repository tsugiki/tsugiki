// `just rust-gates` runs Clippy, the Rust tests with their coverage gate, cargo-deny and cargo-machete, each
// only when the branch changed what CI's job for it reads since the branch left origin/main. scope.ts holds
// the rule, which CI applies too: a branch that changes only `ui/` and docs runs none of them, and one that
// only sets the version runs cargo-deny and cargo-machete alone. Uncommitted and untracked files count, and
// so does each commit on the branch. On main, or when git can't say what changed, every gate runs. Recipes
// named as arguments run in place of those four, as `just preflight` names Clippy and `rust-coverage`.
import { spawnSync } from "node:child_process";

import { git, untracked } from "./run.ts";
import { changedJobs, EVERY_JOB, type Job } from "./scope.ts";

/** The gates that run by default, each with CI's job for it. A gate named but not listed here always runs. */
const JOB_OF: Record<string, Job> = {
  clippy: "rust",
  "rust-coverage": "rust",
  deny: "dependencies",
  machete: "dependencies",
};

const named = process.argv.slice(2);
const GATES = named.length > 0 ? named : Object.keys(JOB_OF);

/** The jobs the branch needs since it left origin/main, or every job on main or when git can't say. */
function branchJobs(): readonly Job[] {
  try {
    if (git("branch", "--show-current") === "main") {
      return EVERY_JOB;
    }
    return changedJobs(git, ["--merge-base", "origin/main"], untracked()).jobs;
  } catch {
    return EVERY_JOB;
  }
}

const needed = branchJobs();
const gates = GATES.filter((gate) => {
  const job = JOB_OF[gate];
  return job === undefined || needed.includes(job);
});
const skipped = GATES.filter((gate) => !gates.includes(gate));
if (skipped.length > 0) {
  console.log(`Skipped the Rust gates (${skipped.join(", ")}): this branch doesn't change what they read.`);
}
if (gates.length > 0) {
  process.exit(spawnSync("just", gates, { stdio: "inherit" }).status ?? 1);
}
