// `just rust-gates` runs Clippy, the Rust tests with their coverage gate, cargo-deny and cargo-machete,
// unless the branch changes only `ui/` and docs since it left origin/main. A branch that only sets the
// version runs cargo-deny and cargo-machete alone. scope.ts holds the rule, which CI applies too.
// Uncommitted and untracked files count, and so does each commit on the branch. On main, or when git can't
// say what changed, every gate runs. Recipes named as arguments run in place of those four, as
// `just preflight` names Clippy and `rust-coverage`.
import { execFileSync, spawnSync } from "node:child_process";

import { paths, type Scope, scope } from "./scope.ts";

const named = process.argv.slice(2);
const GATES = named.length > 0 ? named : ["clippy", "rust-coverage", "deny", "machete"];

function git(...args: string[]): string[] {
  return paths(execFileSync("git", args, { encoding: "utf8" }));
}

/** The gates that read `Cargo.lock`, which a branch that only sets the version still needs. */
const LOCK_GATES = new Set(["deny", "machete"]);

/** The branch's scope since it left origin/main, or `all` on main or when git can't say. */
function branchScope(): Scope {
  try {
    if (git("branch", "--show-current")[0] === "main") {
      return "all";
    }
    const changed = [
      ...git("diff", "--no-renames", "--name-only", "--merge-base", "origin/main"),
      ...git("ls-files", "--others", "--exclude-standard"),
    ];
    const cargoDiff = execFileSync(
      "git",
      ["diff", "-U0", "--merge-base", "origin/main", "--", "Cargo.toml", "Cargo.lock"],
      { encoding: "utf8" },
    );
    return scope(changed, cargoDiff);
  } catch {
    return "all";
  }
}

const branch = branchScope();
const gates =
  branch === "all" ? GATES : branch === "release" ? GATES.filter((gate) => LOCK_GATES.has(gate)) : [];
const skipped = GATES.filter((gate) => !gates.includes(gate));
if (skipped.length > 0) {
  const why = branch === "release" ? "only sets the version" : "changes only ui/ and docs";
  console.log(`Skipped the Rust gates (${skipped.join(", ")}): this branch ${why}.`);
}
if (gates.length > 0) {
  process.exit(spawnSync("just", gates, { stdio: "inherit" }).status ?? 1);
}
