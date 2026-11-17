// CI's `changes` job writes the jobs a run needs to `$GITHUB_OUTPUT`, as `jobs=["rust", ...]`. scope.ts
// holds the rule, which `just rust-gates` applies too. A rename counts as both of its paths.
//
//   node scripts/changes.ts --every
//     On a push to main: every job.
//   node scripts/changes.ts --pr <last-tested.json> <tested.json>
//     On a PR, whose checkout is the merge commit HEAD of base HEAD^1 and the PR's head HEAD^2: the jobs the
//     PR needs against its base, less those whose inputs are the same as in the tree its last run that
//     passed tested. That run left `last-tested.json`, its head and base, and git merges the two again to
//     get its tree. Without the file, or when git can't merge the two, the whole PR counts. It writes this
//     run's head and base to `tested.json`, for the runs after it.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

import { EVERY_JOB, type Job, jobs, narrowed, outputs, paths, readTested } from "./scope.ts";

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** The jobs the change from `from` to the checkout needs. */
function jobsSince(from: string): Job[] {
  return jobs(paths(git("diff", "--no-renames", "--name-only", from, "HEAD")), () =>
    git("diff", "-U0", "--no-color", "--no-ext-diff", from, "HEAD", "--", "Cargo.toml", "Cargo.lock"),
  );
}

/** The jobs the change since the tree that `lastTested` names needs, or `undefined` without that tree. */
function jobsSincePassed(lastTested: string): Job[] | undefined {
  let text: string;
  try {
    text = readFileSync(lastTested, "utf8");
  } catch {
    console.log("No run on this PR has passed yet, or it left no tested.json.");
    return undefined;
  }
  const tested = readTested(text);
  if (tested === undefined) {
    console.log(`${lastTested} doesn't name two commits.`);
    return undefined;
  }
  try {
    const tree = git("merge-tree", "--write-tree", tested.base, tested.head);
    console.log(`The last run that passed tested ${tested.head} on ${tested.base}, tree ${tree}.`);
    return jobsSince(tree);
  } catch {
    console.log(`git can't merge ${tested.head} into ${tested.base} again, so the whole PR counts.`);
    return undefined;
  }
}

const [mode, lastTested, record] = process.argv.slice(2);
const output = process.env["GITHUB_OUTPUT"];
if (output === undefined || !(mode === "--every" || (mode === "--pr" && record !== undefined))) {
  console.error(
    "Usage: `node scripts/changes.ts --every` or `node scripts/changes.ts --pr <last-tested.json> " +
      "<tested.json>`, from a GitHub Actions step.",
  );
  process.exit(1);
}
let needed: Job[] = [...EVERY_JOB];
if (mode === "--pr" && lastTested !== undefined && record !== undefined) {
  const whole = jobsSince("HEAD^1");
  needed = narrowed(whole, jobsSincePassed(lastTested));
  console.log(`Against its base the PR needs ${JSON.stringify(whole)}.`);
  writeFileSync(
    record,
    JSON.stringify({ head: git("rev-parse", "HEAD^2"), base: git("rev-parse", "HEAD^1") }),
  );
}
console.log(`This run needs ${JSON.stringify(needed)}.`);
appendFileSync(output, outputs(needed));
