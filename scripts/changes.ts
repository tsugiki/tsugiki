// CI's `changes` job: `node scripts/changes.ts HEAD^1 HEAD` prints the paths a PR changes and writes
// `docs-only` and `ui-only` to `$GITHUB_OUTPUT`. A rename counts as both of its paths. scope.ts holds the
// rule, which `just rust-gates` applies too.
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";

import { outputs, paths, scope } from "./scope.ts";

const [base, head] = process.argv.slice(2);
const output = process.env["GITHUB_OUTPUT"];
if (base === undefined || head === undefined || output === undefined) {
  console.error("Usage: `node scripts/changes.ts <base> <head>`, from a GitHub Actions step.");
  process.exit(1);
}
const changed = paths(
  execFileSync("git", ["diff", "--no-renames", "--name-only", base, head], { encoding: "utf8" }),
);
console.log(changed.join("\n"));
appendFileSync(output, outputs(scope(changed)));
