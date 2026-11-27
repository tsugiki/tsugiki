// `just merged` tidies up after GitHub merges a PR: it switches to main, pulls, and deletes the PR's local
// branch. GitHub deletes the branch on its side. `just merged <branch>` names the branch, which is otherwise
// the current one. PRs merge by squashing, so git can't tell the branch is merged. The script asks GitHub
// instead, and stops unless the PR is merged and holds the branch's last commit, so a commit that never
// reached the PR stays.
//
// Run on main, as the wave's close runs it, it only pulls, since main is no PR's branch. A branch a worktree
// has checked out stays, with the worktree named: git can't delete it there, and `just worktree-remove`
// deletes both once the branch's PR has merged.
import { execFileSync } from "node:child_process";

import { type Pr, PR_FIELDS, unmergedReason } from "./pr.ts";
import { attempt, run, stop } from "./run.ts";
import { worktreeTicket, worktrees } from "./worktrees.ts";

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
if (branch === "") stop("This checkout is on no branch. Name the PR's branch: `just merged <branch>`.");
if (run("git status failed.", "git", "status", "--porcelain", "--untracked-files=no") !== "") {
  stop("Commit or stash your changes first, or they go along to main.");
}

if (branch !== "main") {
  const tip = run(
    `There's no local branch ${branch}.`,
    "git",
    "rev-parse",
    "--verify",
    `refs/heads/${branch}`,
  );
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
}

step("git", "switch", "main");
step("git", "pull", "--ff-only", "--prune");

if (branch === "main") {
  console.log("Updated main. It is no PR's branch, so there was none to delete.");
} else {
  const holder = worktrees(run("git worktree list failed.", "git", "worktree", "list", "--porcelain")).find(
    (tree) => tree.branch === branch,
  );
  if (holder === undefined) {
    step("git", "branch", "-D", branch);
  } else {
    const ticket = worktreeTicket(holder.path);
    console.log(
      `Kept the branch ${branch}: the worktree ${holder.path} has it checked out. ` +
        (ticket === undefined
          ? "It stays until you remove that worktree."
          : `\`just worktree-remove ${String(ticket)}\` deletes both once its PR has merged.`),
    );
  }
}
