// The decisions behind `just worktree`, `just worktree-remove` and `just merged`, kept apart from the git,
// GitHub and npm calls in worktree.ts and merged.ts so that tests can reach them.

/** Words a branch name leaves out of a ticket's title, as our branches do. */
const FILLER = new Set(
  (
    "a an and are as at be but by each for from in inside into is it its no not of on one or our over so " +
    "than that the their them then this to when which while with without"
  ).split(" "),
);

/** How many words of the title a branch name keeps. */
const SLUG_WORDS = 3;

/** A slug as our branches spell one: lowercase words of letters and digits, joined by hyphens. */
const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** The ticket number `arg` names, or `undefined` when it names none. */
export function ticketNumber(arg: string | undefined): number | undefined {
  return arg !== undefined && /^[1-9]\d*$/.test(arg) ? Number(arg) : undefined;
}

/** The slug a branch takes from a ticket's title: its first few words that aren't filler. */
export function titleSlug(title: string): string {
  const words = title
    .toLowerCase()
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/['’]s\b/g, "")
    .replace(/['’]/g, "")
    .split(/[^a-z0-9]+/)
    .filter((word) => word !== "" && !FILLER.has(word));
  return words.slice(0, SLUG_WORDS).join("-");
}

/**
 * The title of the ticket in `answer`, GitHub's JSON for issue `ticket`. GitHub answers with a PR as well, so
 * it throws on one.
 */
export function ticketTitle(ticket: number, answer: string): string {
  const issue = JSON.parse(answer) as { title?: unknown; pull_request?: unknown };
  if (issue.pull_request !== undefined && issue.pull_request !== null) {
    throw new Error(`#${String(ticket)} is a PR, not a ticket.`);
  }
  if (typeof issue.title !== "string") throw new Error(`GitHub gave #${String(ticket)} no title.`);
  return issue.title;
}

/**
 * The branch for ticket `ticket`: `<ticket>-<slug>`, with the slug the user gave, or else one from the title,
 * which it reads only then. It throws on a slug our branches wouldn't take.
 */
export function ticketBranch(ticket: number, slug: string, title: () => string): string {
  if (slug === "") {
    const read = title();
    const chosen = titleSlug(read);
    if (chosen === "") {
      throw new Error(
        `The title "${read}" gives no slug: name one, as in \`just worktree ${String(ticket)} some-words\`.`,
      );
    }
    return `${String(ticket)}-${chosen}`;
  }
  if (!SLUG.test(slug)) throw new Error(`"${slug}" isn't a slug: use lowercase letters, digits and hyphens.`);
  return `${String(ticket)}-${slug}`;
}

/** The worktree's folder for ticket `ticket`, relative to the main checkout. */
export function worktreeFolder(ticket: number): string {
  return `.temp/wt-${String(ticket)}`;
}

/** One worktree that `git worktree list --porcelain` names. */
export interface Worktree {
  path: string;
  /** Its branch, without `refs/heads/`, or `undefined` when it is detached or bare. */
  branch: string | undefined;
}

/** The worktrees in what `git worktree list --porcelain` prints, in its order. */
export function worktrees(list: string): Worktree[] {
  const trees: Worktree[] = [];
  for (const line of list.split(/\r?\n/)) {
    if (line.startsWith("worktree ")) {
      trees.push({ path: line.slice("worktree ".length), branch: undefined });
    } else if (line.startsWith("branch refs/heads/")) {
      const tree = trees.at(-1);
      if (tree !== undefined) tree.branch = line.slice("branch refs/heads/".length);
    }
  }
  return trees;
}

/** The ticket whose worktree folder `path` is, or `undefined` when it is some other folder. */
export function worktreeTicket(path: string): number | undefined {
  const match = /(?:^|\/)\.temp\/wt-([^/]*)$/.exec(path.replace(/\\/g, "/"));
  return match === null ? undefined : ticketNumber(match[1]);
}

/**
 * What `just merged` prints instead of deleting `branch`, when a worktree `list` names has it checked out, or
 * `undefined` when git can delete the branch.
 */
export function keepReason(list: string, branch: string): string | undefined {
  const holder = worktrees(list).find((tree) => tree.branch === branch);
  if (holder === undefined) return undefined;
  const ticket = worktreeTicket(holder.path);
  return (
    `Kept the branch ${branch}: the worktree ${holder.path} has it checked out. ` +
    (ticket === undefined
      ? "It stays until you remove that worktree."
      : `\`just worktree-remove ${String(ticket)}\` deletes both.`)
  );
}

/**
 * The reason `just worktree` won't make the worktree in `folder`, from what it can tell before it asks git or
 * GitHub anything, or `undefined` when it may go on. `cargoTargetDir` is `CARGO_TARGET_DIR` as the
 * environment sets it.
 */
export function addRefusal(
  cargoTargetDir: string | undefined,
  folder: string,
  folderExists: boolean,
): string | undefined {
  if (cargoTargetDir !== undefined && cargoTargetDir !== "") {
    return (
      `CARGO_TARGET_DIR is set to ${cargoTargetDir}, so each worktree would build into that one folder, ` +
      "against whichever worktree's source built last. Unset it: a worktree builds into its own target/ by " +
      "default."
    );
  }
  if (folderExists) {
    return `${folder} is there already. Remove it with \`just worktree-remove\`, or work in it.`;
  }
  return undefined;
}

/**
 * The reason `just worktree-remove` won't pull main in the main checkout, or `undefined` when it may.
 * `branch` is the main checkout's branch, empty when it has none, and `status` is what
 * `git status --porcelain --untracked-files=no` prints there.
 */
export function pullRefusal(branch: string, status: string): string | undefined {
  if (branch !== "main") {
    return `the main checkout is on ${branch === "" ? "no branch" : branch}, not main.`;
  }
  if (status !== "") return "the main checkout holds changes git hasn't committed.";
  return undefined;
}
