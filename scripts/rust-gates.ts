// `just rust-gates` runs Clippy, the Rust tests with their coverage gate, cargo-deny and cargo-machete,
// unless the branch changes only `ui/` and docs since it left origin/main. scope.ts holds the rule, which CI
// applies too. Uncommitted and untracked files count, and so does each commit on the branch. On main, or
// when git can't say what changed, every gate runs.
import { execFileSync, spawnSync } from "node:child_process";

import { paths, scope } from "./scope.ts";

const GATES = ["clippy", "rust-coverage", "deny", "machete"];

function git(...args: string[]): string[] {
  return paths(execFileSync("git", args, { encoding: "utf8" }));
}

function changedPaths(): string[] {
  try {
    if (git("branch", "--show-current")[0] === "main") {
      return [];
    }
    return [
      ...git("diff", "--no-renames", "--name-only", "--merge-base", "origin/main"),
      ...git("ls-files", "--others", "--exclude-standard"),
    ];
  } catch {
    return [];
  }
}

if (scope(changedPaths()) === "all") {
  process.exit(spawnSync("just", GATES, { stdio: "inherit" }).status ?? 1);
}
console.log(`Skipped the Rust gates (${GATES.join(", ")}): this branch changes only ui/ and docs.`);
