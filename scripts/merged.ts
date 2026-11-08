// `just merged` tidies up after GitHub merges a PR: it switches to main, pulls, and deletes the PR's local
// branch. GitHub deletes the branch on its side. `just merged <branch>` names the branch, which is otherwise
// the current one. PRs merge by squashing, so git can't tell the branch is merged. The script asks GitHub
// instead, and stops unless the PR is merged and holds the branch's last commit, so a commit that never
// reached the PR stays.
import { execFileSync } from "node:child_process";

function stop(message: string): never {
  console.error(message);
  process.exit(1);
}

/** Runs a command and returns its output, or stops with `failed` after the command's own message. */
function run(failed: string, command: string, ...args: string[]): string {
  try {
    return execFileSync(command, args, { encoding: "utf8" }).trim();
  } catch {
    stop(failed);
  }
}

/** Runs a command that prints for the user, and stops if it fails. */
function step(command: string, ...args: string[]): void {
  try {
    execFileSync(command, args, { stdio: "inherit" });
  } catch {
    stop(`\`${command} ${args.join(" ")}\` failed, so the script stopped there.`);
  }
}

function succeeds(command: string, ...args: string[]): boolean {
  try {
    execFileSync(command, args, { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

const branch =
  process.argv[2] || run("git can't name the current branch.", "git", "branch", "--show-current");
if (branch === "" || branch === "main") stop("Name the PR's branch: `just merged <branch>`.");
const tip = run(`There's no local branch ${branch}.`, "git", "rev-parse", "--verify", `refs/heads/${branch}`);
if (run("git status failed.", "git", "status", "--porcelain", "--untracked-files=no") !== "") {
  stop("Commit or stash your changes first, or they go along to main.");
}

interface Pr {
  number: number;
  state: string;
  headRefName: string;
  headRefOid: string;
}
const pr = JSON.parse(
  run(
    `gh couldn't read the PR for ${branch}. Its message is above.`,
    "gh",
    "pr",
    "view",
    branch,
    "--json",
    "number,state,headRefName,headRefOid",
  ),
) as Pr;
if (pr.headRefName !== branch) stop(`gh found #${pr.number}, whose branch is ${pr.headRefName}.`);
if (pr.state !== "MERGED") stop(`#${pr.number} is ${pr.state.toLowerCase()}, not merged.`);
if (tip !== pr.headRefOid && !succeeds("git", "merge-base", "--is-ancestor", tip, pr.headRefOid)) {
  stop(`${branch} is at ${tip.slice(0, 7)}, which #${pr.number} doesn't hold.`);
}

step("git", "switch", "main");
step("git", "pull", "--ff-only", "--prune");
step("git", "branch", "-D", branch);
