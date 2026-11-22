// `node scripts/gates.ts <recipe>...` runs each recipe named, as `just` does, but skips a gate CI skips: one
// whose CI job reads nothing the branch changed since it left origin/main. scope.ts holds the rule, which CI
// applies too: a branch that changes only `ui/` and docs runs no Rust gate, one that only sets the version
// runs cargo-deny and cargo-machete alone, and one that changes only scripts runs no Playwright spec.
// Uncommitted and untracked files count, and so does each commit on the branch. On main, or when git can't
// say what changed, every gate runs. `just rust-gates`, `just check` and `just preflight` call it.
import { spawnSync } from "node:child_process";

import { git, untracked } from "./run.ts";
import { changedJobs, EVERY_JOB, type Job } from "./scope.ts";

/** The gates CI can skip, each with CI's job for it. A recipe named but not listed here always runs. */
const JOB_OF: Record<string, Job> = {
  clippy: "rust",
  "rust-coverage": "rust",
  deny: "dependencies",
  machete: "dependencies",
  e2e: "e2e",
};

const GATES = process.argv.slice(2);
if (GATES.length === 0) {
  console.error("Name the recipes to run: `node scripts/gates.ts clippy e2e`.");
  process.exit(1);
}

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
  console.log(`Skipped ${skipped.join(", ")}: this branch doesn't change what CI's job for it reads.`);
}
if (gates.length > 0) {
  process.exit(spawnSync("just", gates, { stdio: "inherit" }).status ?? 1);
}
