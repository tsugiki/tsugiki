// `just worktree <ticket> [slug]` makes a worktree ready to build a ticket, as a wave's agent does. It makes
// `.temp/wt-<ticket>` in the main checkout, on a new branch `<ticket>-<slug>` off origin/main that tracks
// nothing, so a push can't land on main. The slug comes from the ticket's title unless you name one. It
// copies the main checkout's target/tools, so the Rust tests find ty and clangd without fetching them again,
// and runs `npm ci` there, as vitest would otherwise load the main checkout's packages. The worktree builds
// into its own target/, Cargo's default, and the script refuses to run while CARGO_TARGET_DIR is set.
//
// `just worktree-remove <ticket>` deletes the worktree, its target/ and its node_modules, unless it holds a
// change git hasn't committed. It deletes the branch too once the branch's PR has merged with its last
// commit, which it asks GitHub, as `just merged` does, and then pulls main in the main checkout when that
// checkout is on main with nothing uncommitted.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, realpathSync, rmSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";

import { type Pr, PR_FIELDS, unmergedReason } from "./pr.ts";
import { attempt, messageOf, run, stop } from "./run.ts";
import {
  addRefusal,
  pullRefusal,
  ticketBranch,
  ticketNumber,
  ticketTitle,
  worktreeFolder,
  worktrees,
} from "./worktrees.ts";

/** The main checkout, whichever checkout of the repository the script runs in. */
function mainCheckout(): string {
  const common = run(
    "This isn't a git checkout.",
    "git",
    "rev-parse",
    "--path-format=absolute",
    "--git-common-dir",
  );
  return dirname(resolve(common));
}

/** Whether two paths are one, as git and Node may spell them differently. */
function samePath(a: string, b: string): boolean {
  const [x, y] = [resolve(a), resolve(b)];
  return process.platform === "win32" ? x.toLowerCase() === y.toLowerCase() : x === y;
}

/** Whether two paths name one folder that exists, through any links on the way. */
function sameFolder(a: string, b: string): boolean {
  return samePath(realpathSync(a), realpathSync(b));
}

/** Whether git lists a worktree at `path`. */
function listed(main: string, path: string): boolean {
  const list = run("git worktree list failed.", "git", "-C", main, "worktree", "list", "--porcelain");
  return worktrees(list).some((tree) => samePath(tree.path, path));
}

/** Deletes `path` and all it holds, or stops with what may hold it and how to finish. */
function deleteFolder(path: string, ticket: number): void {
  try {
    rmSync(path, { recursive: true, force: true, maxRetries: 3 });
  } catch (cause) {
    const code = (cause as NodeJS.ErrnoException).code ?? String(cause);
    stop(
      `Couldn't delete ${path} (${code}). A build, a test run, a language server, an editor or a terminal ` +
        `may hold a file in it: stop it, then run \`just worktree-remove ${String(ticket)}\` again.`,
    );
  }
}

function add(ticket: number, slug: string): void {
  const main = mainCheckout();
  const folder = worktreeFolder(ticket);
  const path = join(main, folder);
  const refusal = addRefusal(process.env.CARGO_TARGET_DIR, folder, existsSync(path));
  if (refusal !== undefined) stop(refusal);
  let branch: string;
  try {
    branch = ticketBranch(ticket, slug, () =>
      ticketTitle(
        ticket,
        run(
          `gh couldn't read ticket #${String(ticket)}. Its message is above.`,
          "gh",
          "api",
          `repos/{owner}/{repo}/issues/${String(ticket)}`,
        ),
      ),
    );
  } catch (cause) {
    stop(messageOf(cause));
  }
  if (attempt("git", "-C", main, "rev-parse", "--verify", "-q", `refs/heads/${branch}`) !== undefined) {
    stop(`The branch ${branch} is there already. Delete it, or name another slug.`);
  }

  run("git fetch failed.", "git", "-C", main, "fetch", "-q", "origin", "main");
  run(
    `git couldn't make the worktree ${folder}.`,
    "git",
    "-C",
    main,
    "worktree",
    "add",
    "-q",
    "--no-track",
    "-b",
    branch,
    path,
    "origin/main",
  );
  const tools = join(main, "target", "tools");
  if (existsSync(tools)) {
    // A folder test-servers.ts left half unpacked would only be unpacked again.
    cpSync(tools, join(path, "target", "tools"), {
      recursive: true,
      filter: (source) => !source.endsWith(".unpacking"),
    });
  }
  // npm is a .cmd script on Windows, which only a shell starts.
  const npm = spawnSync("npm ci", { cwd: path, stdio: "inherit", shell: true });
  if (npm.status !== 0) {
    stop(`\`npm ci\` failed in ${path}, on the branch ${branch}. Run it there again to finish.`);
  }
  console.log(`The worktree ${path} is on the branch ${branch}.`);
}

function remove(ticket: number): void {
  const main = realpathSync(mainCheckout());
  const folder = worktreeFolder(ticket);
  const path = join(main, folder);
  if (!existsSync(path)) stop(`There's no worktree ${folder}.`);
  const resolved = realpathSync(path);
  if (!samePath(path, resolved)) {
    stop(`Expected ${path}, but ${folder} resolves to ${resolved}.`);
  }
  const here = relative(path, realpathSync(process.cwd()));
  if (!here.startsWith("..") && !isAbsolute(here)) {
    stop(`Run it from outside ${folder}, which it deletes.`);
  }
  // Without its .git file, git would read the main checkout from inside the folder.
  const top = attempt("git", "-C", path, "rev-parse", "--show-toplevel");
  if (top === undefined || !sameFolder(top, path)) {
    run("git worktree prune failed.", "git", "-C", main, "worktree", "prune");
    if (existsSync(join(path, ".git")) || listed(main, path)) {
      stop(`${folder} isn't a worktree git can read. Look inside, then delete it by hand.`);
    }
    deleteFolder(path, ticket);
    console.log(`Deleted ${path}, which a removal left behind. Delete its branch by hand.`);
    return;
  }
  if (run(`git status failed in ${folder}.`, "git", "-C", path, "status", "--porcelain") !== "") {
    stop(`${folder} holds changes git hasn't committed. Commit them, or delete them, first.`);
  }
  const branch = run(`git can't name ${folder}'s branch.`, "git", "-C", path, "branch", "--show-current");
  const tip = run(`git can't read ${folder}'s last commit.`, "git", "-C", path, "rev-parse", "HEAD");

  // Node deletes the build folders, whose paths can run longer than git on Windows can delete, and git then
  // deletes the rest along with its record of the worktree.
  for (const built of ["target", "node_modules", join("ui", "node_modules")]) {
    deleteFolder(join(path, built), ticket);
  }
  run(
    `git couldn't remove ${folder}, as it says above. Stop what holds a file in it, then run ` +
      `\`just worktree-remove ${String(ticket)}\` again.`,
    "git",
    "-C",
    main,
    "worktree",
    "remove",
    path,
  );
  console.log(`Removed the worktree ${path}.`);
  if (branch === "") return;

  const view = spawnSync("gh", ["pr", "view", branch, "--json", PR_FIELDS], { encoding: "utf8" });
  if (view.status !== 0) {
    if (/no pull requests found/i.test(view.stderr)) {
      console.log(`Kept the branch ${branch}, which has no PR.`);
      return;
    }
    console.error(view.stderr.trim() || String(view.error));
    stop(`Kept the branch ${branch}: gh couldn't read its PR, as it says above.`);
  }
  const pr = JSON.parse(view.stdout) as Pr;
  const unmerged = unmergedReason(
    branch,
    tip,
    pr,
    (commit, of) => attempt("git", "-C", main, "merge-base", "--is-ancestor", commit, of) !== undefined,
  );
  if (unmerged !== undefined) {
    console.log(`Kept the branch ${branch}: ${unmerged}`);
    return;
  }
  run(`git couldn't delete the branch ${branch}.`, "git", "-C", main, "branch", "-q", "-D", branch);
  console.log(`Deleted the branch ${branch}.`);

  const refusal = pullRefusal(
    run("git can't name the main checkout's branch.", "git", "-C", main, "branch", "--show-current"),
    run(
      "git status failed in the main checkout.",
      "git",
      "-C",
      main,
      "status",
      "--porcelain",
      "--untracked-files=no",
    ),
  );
  if (refusal !== undefined) {
    console.log(`Didn't pull main: ${refusal}`);
    return;
  }
  run("git couldn't pull main, as it says above.", "git", "-C", main, "pull", "-q", "--ff-only", "--prune");
  console.log("Pulled main.");
}

const [command, number, slug = "", ...extra] = process.argv.slice(2);
const ticket = ticketNumber(number);
if (command === "add" && ticket !== undefined && extra.length === 0) {
  add(ticket, slug);
} else if (command === "remove" && ticket !== undefined && slug === "" && extra.length === 0) {
  remove(ticket);
} else {
  stop("Call it as `just worktree <ticket> [slug]` or `just worktree-remove <ticket>`.");
}
