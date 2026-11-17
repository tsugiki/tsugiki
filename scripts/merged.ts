// `just merged` tidies up after GitHub merges a PR: it switches to main, pulls, and deletes the PR's local
// branch. GitHub deletes the branch on its side. `just merged <branch>` names the branch, which is otherwise
// the current one. PRs merge by squashing, so git can't tell the branch is merged. The script asks GitHub
// instead, and stops unless the PR is merged and holds the branch's last commit, so a commit that never
// reached the PR stays.
import { execFileSync } from "node:child_process";

import { type Pr, PR_FIELDS, unmergedReason } from "./pr.ts";
import { attempt, run, stop } from "./run.ts";

/** Runs a command that prints for the user, and stops if it fails. */
function step(command: string, ...args: string[]): void {
  try {
    execFileSync(command, args, { stdio: "inherit" });
  } catch {
    stop(`\`${command} ${args.join(" ")}\` failed, so the script stopped there.`);
  }
}

const branch =
  process.argv[2] || run("git can't name the current branch.", "git", "branch", "--show-current");
if (branch === "" || branch === "main") stop("Name the PR's branch: `just merged <branch>`.");
const tip = run(`There's no local branch ${branch}.`, "git", "rev-parse", "--verify", `refs/heads/${branch}`);
if (run("git status failed.", "git", "status", "--porcelain", "--untracked-files=no") !== "") {
  stop("Commit or stash your changes first, or they go along to main.");
}

const pr = JSON.parse(
  run(
    `gh couldn't read the PR for ${branch}. Its message is above.`,
    "gh",
    "pr",
    "view",
    branch,
    "--json",
    PR_FIELDS,
  ),
) as Pr;
const unmerged = unmergedReason(
  branch,
  tip,
  pr,
  (commit, of) => attempt("git", "merge-base", "--is-ancestor", commit, of) !== undefined,
);
if (unmerged !== undefined) stop(unmerged);

step("git", "switch", "main");
step("git", "pull", "--ff-only", "--prune");
step("git", "branch", "-D", branch);
