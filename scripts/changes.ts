// Tells a workflow which of its checks a run needs, by a line it writes to `$GITHUB_OUTPUT`. scope.ts holds
// the rules, which `just check` applies too. A rename counts as both of its paths. On a PR the checkout
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
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";

import { git, stop } from "./run.ts";
import {
  cargoDiffArgs,
  changedJobs,
  changedPaths,
  EVERY_JOB,
  type Job,
  narrowed,
  needsRustBenchmarks,
  outputs,
  sincePassed,
} from "./scope.ts";

/** The PR's checkout against its base. */
const AGAINST_BASE = ["HEAD^1", "HEAD"];

/** The file's text, or `undefined` when it can't be read. */
function readIfThere(file: string): string | undefined {
  try {
    return readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
}

/** Logs `changed`, the paths the PR changes against its base, and passes them on. */
function logged(changed: string[]): string[] {
  console.log(
    `Against its base the PR changes ${changed.length > 0 ? changed.join(", ") : "nothing git can name"}.`,
  );
  return changed;
}

/** The jobs this PR's run needs, and writes what it tests to `record`. */
function prJobs(lastTested: string, record: string): Job[] {
  const whole = changedJobs(git, AGAINST_BASE);
  logged(whole.paths);
  console.log(`The whole PR needs ${JSON.stringify(whole.jobs)}.`);
  const since = sincePassed(readIfThere(lastTested), git);
  console.log(
    since.jobs === undefined ? since.note : `${since.note} That needs ${JSON.stringify(since.jobs)}.`,
  );
  writeFileSync(
    record,
    JSON.stringify({ head: git("rev-parse", "HEAD^2"), base: git("rev-parse", "HEAD^1") }),
  );
  return narrowed(whole.jobs, since.jobs);
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
  const changed = logged(changedPaths(git, AGAINST_BASE));
  line = benchmarksLine(needsRustBenchmarks(changed, () => git(...cargoDiffArgs(...AGAINST_BASE))));
} else {
  stop(
    "Usage: `node scripts/changes.ts` with `--every`, `--pr <last-tested.json> <tested.json>`, " +
      "`--all-benchmarks` or `--benchmarks`, from a GitHub Actions step.",
  );
}
appendFileSync(output, line);
