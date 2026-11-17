// Tells a workflow which of its checks a run needs, by a line it writes to `$GITHUB_OUTPUT`. scope.ts holds
// the rules, which `just rust-gates` applies too. A rename counts as both of its paths. On a PR the checkout
// is the merge commit HEAD of the base HEAD^1 and the PR's head HEAD^2.
//
// CI's `changes` job writes the jobs to run, as `jobs=["rust", ...]`:
//
//   node scripts/changes.ts --every
//     On a push to main: every job.
//   node scripts/changes.ts --pr <last-tested.json> <tested.json>
//     On a PR: the jobs the PR needs against its base, less those whose inputs are the same as in the tree
//     its last passed run tested (scope.ts's `sincePassed`). That run left `last-tested.json`. It writes
//     this run's head and base to `tested.json`, for the runs after it.
//
// The benchmarks' `scope` step writes whether tsugiki-core's benchmarks run, as `rust=true` or `rust=false`:
//
//   node scripts/changes.ts --all-benchmarks
//     On a push to main or a run by hand: true, which keeps CodSpeed's baseline whole.
//   node scripts/changes.ts --benchmarks
//     On a PR: whether the whole PR changes what they read against its base, not since its last run.
//     CodSpeed fills a benchmark a run skips with the base's figure, so a run can skip only what the PR
//     doesn't change.
import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

import {
  cargoDiffArgs,
  EVERY_JOB,
  type Job,
  jobs,
  narrowed,
  needsRustBenchmarks,
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

/** The paths the PR changes against its base, which it logs. */
function changedAgainstBase(): string[] {
  const changed = paths(git("diff", "--no-renames", "--name-only", "HEAD^1", "HEAD"));
  console.log(
    `Against its base the PR changes ${changed.length > 0 ? changed.join(", ") : "nothing git can name"}.`,
  );
  return changed;
}

/** The jobs this PR's run needs, and writes what it tests to `record`. */
function prJobs(lastTested: string, record: string): Job[] {
  const whole = jobs(changedAgainstBase(), () => cargoDiffSince("HEAD^1"));
  console.log(`The whole PR needs ${JSON.stringify(whole)}.`);
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

/** The line for CI's jobs. */
function jobsLine(needed: Job[]): string {
  console.log(`This run needs ${JSON.stringify(needed)}.`);
  return outputs(needed);
}

/** The line for the benchmarks. */
function benchmarksLine(rust: boolean): string {
  console.log(rust ? "tsugiki-core's benchmarks run." : "tsugiki-core's benchmarks skip.");
  return `rust=${rust}\n`;
}

const [mode, lastTested, record] = process.argv.slice(2);
const output = process.env["GITHUB_OUTPUT"];
let line: string;
if (output !== undefined && mode === "--every") {
  line = jobsLine([...EVERY_JOB]);
} else if (output !== undefined && mode === "--pr" && lastTested !== undefined && record !== undefined) {
  line = jobsLine(prJobs(lastTested, record));
} else if (output !== undefined && mode === "--all-benchmarks") {
  line = benchmarksLine(true);
} else if (output !== undefined && mode === "--benchmarks") {
  line = benchmarksLine(needsRustBenchmarks(changedAgainstBase(), () => cargoDiffSince("HEAD^1")));
} else {
  console.error(
    "Usage: `node scripts/changes.ts` with `--every`, `--pr <last-tested.json> <tested.json>`, " +
      "`--all-benchmarks` or `--benchmarks`, from a GitHub Actions step.",
  );
  process.exit(1);
}
appendFileSync(output, line);
