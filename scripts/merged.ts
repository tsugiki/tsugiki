// `just merged` tidies up after GitHub merges a PR: it switches to main, pulls, and deletes the PR's branch.
// `just merged <branch>` names the branch, which is otherwise the current one. PRs merge by squashing, so
// git can't tell the branch is merged. The script asks GitHub instead, and stops unless the PR is merged and
// its head is the branch's last commit, so a commit that never reached the PR stays.
import { execFileSync } from "node:child_process";

function run(command: string, ...args: string[]): string {
  return execFileSync(command, args, { encoding: "utf8" }).trim();
}

function stop(message: string): never {
  console.error(message);
  process.exit(1);
}

const branch = process.argv[2] ?? run("git", "branch", "--show-current");
if (branch === "" || branch === "main") stop("Name the PR's branch: `just merged <branch>`.");

interface Pr {
  number: number;
  state: string;
  headRefOid: string;
}

let pr: Pr;
try {
  pr = JSON.parse(run("gh", "pr", "view", branch, "--json", "number,state,headRefOid")) as Pr;
} catch {
  stop(`gh found no PR for ${branch}.`);
}
if (pr.state !== "MERGED") stop(`#${pr.number} is ${pr.state.toLowerCase()}, not merged.`);
const tip = run("git", "rev-parse", branch);
if (tip !== pr.headRefOid) {
  stop(`${branch} is at ${tip.slice(0, 7)}, but #${pr.number} merged ${pr.headRefOid.slice(0, 7)}.`);
}

execFileSync("git", ["switch", "main"], { stdio: "inherit" });
execFileSync("git", ["pull", "--ff-only", "--prune"], { stdio: "inherit" });
execFileSync("git", ["branch", "-D", branch], { stdio: "inherit" });
