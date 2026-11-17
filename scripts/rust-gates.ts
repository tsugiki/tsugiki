// `just rust-gates` runs Clippy, the Rust tests with their coverage gate, cargo-deny and cargo-machete, each
// only when the branch changed what CI's job for it reads since the branch left origin/main. scope.ts holds
// the rule, which CI applies too: a branch that changes only `ui/` and docs runs none of them, and one that
// only sets the version runs cargo-deny and cargo-machete alone. Uncommitted and untracked files count, and
// so does each commit on the branch. On main, or when git can't say what changed, every gate runs. Recipes
// named as arguments run in place of those four, as `just preflight` names Clippy and `rust-coverage`.
import { execFileSync, spawnSync } from "node:child_process";

import { EVERY_JOB, type Job, jobs, paths } from "./scope.ts";

const named = process.argv.slice(2);
const GATES = named.length > 0 ? named : ["clippy", "rust-coverage", "deny", "machete"];

/** CI's job for each gate. A gate it doesn't name always runs. */
const JOB_OF: Record<string, Job> = {
  clippy: "rust",
  "rust-coverage": "rust-coverage",
  deny: "dependencies",
  machete: "dependencies",
};

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8" });
}

/** The jobs the branch needs since it left origin/main, or every job on main or when git can't say. */
function branchJobs(): readonly Job[] {
  try {
    if (paths(git("branch", "--show-current"))[0] === "main") {
      return EVERY_JOB;
    }
    const changed = [
      ...paths(git("diff", "--no-renames", "--name-only", "--merge-base", "origin/main")),
      ...paths(git("ls-files", "--others", "--exclude-standard")),
    ];
    const cargoDiff = (): string =>
      git(
        "diff",
        "-U0",
        "--no-color",
        "--no-ext-diff",
        "--merge-base",
        "origin/main",
        "--",
        "Cargo.toml",
        "Cargo.lock",
      );
    return jobs(changed, cargoDiff);
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
