// `just rust-gates` runs Clippy, the Rust tests with their coverage gate, cargo-deny and cargo-machete,
// unless the branch changes only `ui/` and docs since it left main. scope.ts holds the rule, which CI applies
// too.
// Uncommitted and untracked files count, and so does each commit on the branch. On main, or when git can't
// say what changed, every gate runs.
import { execFileSync, spawnSync } from "node:child_process";

import { scope } from "./scope.ts";

const GATES = ["clippy", "rust-coverage", "deny", "machete"];

function lines(...args: string[]): string[] {
  return execFileSync("git", args, { encoding: "utf8" })
    .split("\n")
    .filter((line) => line !== "");
}

function changedPaths(): string[] {
  try {
    if (lines("branch", "--show-current")[0] === "main") {
      return [];
    }
    const [base] = lines("merge-base", "main", "HEAD");
    if (base === undefined) {
      return [];
    }
    return [
      ...lines("diff", "--no-renames", "--name-only", base),
      ...lines("ls-files", "--others", "--exclude-standard"),
    ];
  } catch {
    return [];
  }
}

if (scope(changedPaths()) === "all") {
  process.exit(spawnSync("just", GATES, { stdio: "inherit" }).status ?? 1);
}
console.log(`Skipped the Rust gates (${GATES.join(", ")}): this branch changes only ui/ and docs.`);
