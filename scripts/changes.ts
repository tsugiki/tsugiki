// CI's `changes` job writes the jobs a run needs to `$GITHUB_OUTPUT`, as `jobs=["rust", ...]`. scope.ts
// holds the rules, which `just rust-gates` applies too. A rename counts as both of its paths.
//
//   node scripts/changes.ts --every
//     On a push to main: every job.
//   node scripts/changes.ts --pr <last-tested.json> <tested.json>
//     On a PR, whose checkout is the merge commit HEAD of base HEAD^1 and the PR's head HEAD^2: the jobs the
//     PR needs against its base, less those whose inputs are the same as in the tree its last passed run
//     tested (scope.ts's `sincePassed`). That run left `last-tested.json`. It writes this run's head and base
//     to `tested.json`, for the runs after it.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

import {
  cargoDiffArgs,
  EVERY_JOB,
  type Job,
  jobs,
  narrowed,
  outputs,
  paths,
  sincePassed,
  type TreeGit,
} from "./scope.ts";

function git(...args: string[]): string {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** What `git diff -U0` prints for the Cargo files from `from` to the checkout, or "" when git can't say. */
function cargoDiffSince(from: string): string {
  try {
    return git(...cargoDiffArgs(from, "HEAD"));
  } catch {
    return "";
  }
}

const TREE_GIT: TreeGit = {
  mergeTree: (base, head) => git("merge-tree", "--write-tree", base, head),
  changedSince: (tree) => paths(git("diff", "--no-renames", "--name-only", tree, "HEAD")),
  cargoDiffSince,
};

/** The file's text, or `undefined` when it can't be read. */
function readIfThere(file: string): string | undefined {
  try {
    return readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
}

/** The jobs this PR's run needs, and writes what it tests to `record`. */
function prJobs(lastTested: string, record: string): Job[] {
  const changed = paths(git("diff", "--no-renames", "--name-only", "HEAD^1", "HEAD"));
  const whole = jobs(changed, () => cargoDiffSince("HEAD^1"));
  const listed = changed.length > 0 ? changed.join(", ") : "nothing git can name";
  console.log(`Against its base the PR changes ${listed}, and needs ${JSON.stringify(whole)}.`);
  const since = sincePassed(readIfThere(lastTested), TREE_GIT);
  console.log(
    since.jobs === undefined ? since.note : `${since.note} That needs ${JSON.stringify(since.jobs)}.`,
  );
  writeFileSync(
    record,
    JSON.stringify({ head: git("rev-parse", "HEAD^2"), base: git("rev-parse", "HEAD^1") }),
  );
  return narrowed(whole, since.jobs);
}

const [mode, lastTested, record] = process.argv.slice(2);
const output = process.env["GITHUB_OUTPUT"];
let needed: Job[];
if (output !== undefined && mode === "--every") {
  needed = [...EVERY_JOB];
} else if (output !== undefined && mode === "--pr" && lastTested !== undefined && record !== undefined) {
  needed = prJobs(lastTested, record);
} else {
  console.error(
    "Usage: `node scripts/changes.ts --every` or `node scripts/changes.ts --pr <last-tested.json> " +
      "<tested.json>`, from a GitHub Actions step.",
  );
  process.exit(1);
}
console.log(`This run needs ${JSON.stringify(needed)}.`);
appendFileSync(output, outputs(needed));
