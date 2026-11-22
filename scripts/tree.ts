// The layout of `just tree`: the open tickets below some tickets, as a tree of their blockers, so you can see
// how deep a chain runs and where to cut it so more of its tickets can be built at once. A ticket prints
// under the open blocker on its longest chain, and its other open blockers follow it in `[also #N]`. With
// `--up` it draws the other way: the open blockers above some tickets, each blocker under the ticket it
// blocks on its longest chain from the ticket named.
import { BLOCKED_BY, type BlockedBy, cutWarning, type OpenBlockers, openBlockers } from "./blockers.ts";

/** An open issue, with its open blockers. */
export interface TreeTicket extends OpenBlockers {
  number: number;
  title: string;
  /** Its milestone's title, if it has one. */
  milestone: string | undefined;
}

// Every open issue of the repo, a page of 100 at a time. `gh api graphql --paginate` fills only a variable
// named `$endCursor`: under any other name it asks for the first page again, forever.
export const QUERY = `query($owner: String!, $name: String!, $endCursor: String) {
  repository(owner: $owner, name: $name) {
    nameWithOwner
    issues(states: OPEN, first: 100, after: $endCursor) {
      nodes {
        number
        title
        milestone { title }
        ${BLOCKED_BY}
      }
      pageInfo { hasNextPage endCursor }
    }
  }
}`;

interface Page {
  data: {
    repository: {
      nameWithOwner: string;
      issues: {
        nodes: { number: number; title: string; milestone: { title: string } | null; blockedBy: BlockedBy }[];
      };
    };
  };
}

/**
 * The open issues in what `gh api graphql --paginate --slurp` prints for `QUERY`, a JSON array of pages, and
 * the warning to print if GitHub listed only some of an issue's blockers.
 */
export function ticketsFromPages(reply: string): { tickets: TreeTicket[]; warning: string | undefined } {
  const repositories = (JSON.parse(reply) as Page[]).map((page) => page.data.repository);
  const tickets = repositories.flatMap((repository) =>
    repository.issues.nodes.map((issue) => ({
      number: issue.number,
      title: issue.title,
      milestone: issue.milestone?.title,
      ...openBlockers(issue.blockedBy, repository.nameWithOwner),
    })),
  );
  return { tickets, warning: cutWarning(repositories.flatMap((repository) => repository.issues.nodes)) };
}

/** Whether a ticket has an open blocker whose own blockers we don't know, so it counts 1 to the depth. */
function blockedOutside(ticket: TreeTicket, byNumber: Map<number, TreeTicket>): boolean {
  return ticket.blockersElsewhere.length > 0 || ticket.openBlockers.some((n) => !byNumber.has(n));
}

/**
 * A ticket's line: its milestone, its depth, and in `[also]` the open blockers `others` and those in other
 * repositories, with a star on each one no tree `drawn` holds.
 */
function lineOf(ticket: TreeTicket, depth: number, others: number[], drawn: Set<number>): string {
  const milestone = ticket.milestone === undefined ? "" : `[${ticket.milestone}] `;
  const also = [
    ...[...others]
      .sort((a, b) => a - b)
      .map((blocker) => `#${String(blocker)}${drawn.has(blocker) ? "" : "*"}`),
    ...ticket.blockersElsewhere.map((blocker) => `${blocker}*`),
  ];
  const tail = also.length === 0 ? "" : `  [also ${also.join(", ")}]`;
  return `#${String(ticket.number)} ${milestone}${ticket.title} (${String(depth)})${tail}`;
}

/** The lines of the trees whose tops are `roots`, with the children of each ticket, and the deepest last. */
function drawTrees(
  roots: number[],
  children: Map<number, number[]>,
  depth: (number: number) => number,
  line: (number: number, above: number | undefined) => string,
): string[] {
  const lines: string[] = [];
  let deepest = 0;
  const print = (number: number, lead: string, indent: string, above?: number): void => {
    lines.push(lead + line(number, above));
    deepest = Math.max(deepest, depth(number));
    const below = (children.get(number) ?? []).sort((a, b) => depth(a) - depth(b) || a - b);
    below.forEach((child, at) => {
      const last = at === below.length - 1;
      print(child, `${indent}${last ? "└─ " : "├─ "}`, `${indent}${last ? "   " : "│  "}`, number);
    });
  };
  for (const root of roots) {
    print(root, "", "");
  }
  return lines.length === 0 ? [] : [...lines, `max depth: ${String(deepest)}`];
}

/**
 * The lines `just tree` prints for the open `tickets`: one tree for each ticket `named`, in that order, or
 * with none named, one for each ticket with no open blocker. Each ticket named must be in `tickets`.
 */
export function treeLines(tickets: TreeTicket[], named: number[]): string[] {
  const byNumber = new Map(tickets.map((ticket) => [ticket.number, ticket]));

  // Each ticket's depth: 1 plus the depth of its deepest open blocker, in the trees or not. A blocker in
  // another repository, or one GitHub didn't list as an open issue here, has no blockers we know of, so it
  // counts 1. GitHub's docs don't say it refuses a cycle of blockers, so a walk from the lowest number drops
  // the blocker that would close one. The tree hangs from the blockers `kept`, and `[also]` still names the
  // dropped one.
  const kept = new Map<number, number[]>();
  const depths = new Map<number, number>();
  const walking = new Set<number>();
  const walk = (ticket: TreeTicket): number => {
    const known = depths.get(ticket.number);
    if (known !== undefined) {
      return known;
    }
    walking.add(ticket.number);
    const blockers = [...ticket.openBlockers]
      .sort((a, b) => a - b)
      .flatMap((number) => {
        const blocker = byNumber.get(number);
        return blocker !== undefined && !walking.has(number) ? [blocker] : [];
      });
    const result = 1 + Math.max(blockedOutside(ticket, byNumber) ? 1 : 0, ...blockers.map(walk));
    walking.delete(ticket.number);
    kept.set(
      ticket.number,
      blockers.map((blocker) => blocker.number),
    );
    depths.set(ticket.number, result);
    return result;
  };
  for (const ticket of [...tickets].sort((a, b) => a.number - b.number)) {
    walk(ticket);
  }
  const depth = (number: number): number => depths.get(number) as number;
  const dependents = new Map<number, number[]>();
  for (const [number, blockers] of kept) {
    for (const blocker of blockers) {
      dependents.set(blocker, [...(dependents.get(blocker) ?? []), number]);
    }
  }

  const roots =
    named.length > 0
      ? [...new Set(named)]
      : [...kept]
          .filter(([, blockers]) => blockers.length === 0)
          .map(([number]) => number)
          .sort((a, b) => a - b);
  const isRoot = new Set(roots);
  const inTrees = new Set(roots);
  for (const number of inTrees) {
    for (const dependent of dependents.get(number) ?? []) {
      inTrees.add(dependent);
    }
  }

  // Each ticket below a root goes under its deepest blocker in the trees, the lowest number on a tie. The
  // walk reached it through one, and `kept` is in order of number.
  const children = new Map<number, number[]>();
  for (const [number, blockers] of kept) {
    if (inTrees.has(number) && !isRoot.has(number)) {
      const parent = blockers
        .filter((blocker) => inTrees.has(blocker))
        .reduce((best, blocker) => (depth(blocker) > depth(best) ? blocker : best));
      children.set(parent, [...(children.get(parent) ?? []), number]);
    }
  }

  return drawTrees(roots, children, depth, (number, above) => {
    const ticket = byNumber.get(number) as TreeTicket;
    const others = ticket.openBlockers.filter((blocker) => blocker !== above);
    return lineOf(ticket, depth(number), others, inTrees);
  });
}

/**
 * The lines `just tree --up` prints for the open `tickets`: one tree for each ticket `named`, in that order,
 * with each ticket's open blockers under it. Each ticket named must be in `tickets`.
 */
export function blockerLines(tickets: TreeTicket[], named: number[]): string[] {
  const byNumber = new Map(tickets.map((ticket) => [ticket.number, ticket]));
  const ticketOf = (number: number): TreeTicket => byNumber.get(number) as TreeTicket;
  const roots = [...new Set(named)];
  const isRoot = new Set(roots);

  // A walk up from the tickets named reaches each open blocker here. A blocker still on the walk's path
  // would close a cycle, so the walk doesn't follow it, and `[also]` names it. `climb` keeps the rest, and
  // `order` lists each ticket after its blockers.
  const climb = new Map<number, number[]>();
  const order: number[] = [];
  const walking = new Set<number>();
  const visit = (number: number): void => {
    walking.add(number);
    const next = ticketOf(number)
      .openBlockers.filter((blocker) => byNumber.has(blocker) && !walking.has(blocker))
      .sort((a, b) => a - b);
    climb.set(number, next);
    for (const blocker of next) {
      if (!climb.has(blocker)) {
        visit(blocker);
      }
    }
    walking.delete(number);
    order.push(number);
  };
  for (const root of roots) {
    if (!climb.has(root)) {
      visit(root);
    }
  }

  // Each ticket's depth: 1 plus the depth of its deepest blocker the walk kept, as `treeLines` counts it.
  const depths = new Map<number, number>();
  for (const number of order) {
    const below = (climb.get(number) as number[]).map((blocker) => depths.get(blocker) as number);
    depths.set(number, 1 + Math.max(blockedOutside(ticketOf(number), byNumber) ? 1 : 0, ...below));
  }
  const depth = (number: number): number => depths.get(number) as number;

  // Each blocker goes under the ticket it blocks that sits furthest from a root, the lowest number on a tie,
  // so the tree shows its longest chain. `order` reversed reaches each blocker after every ticket it blocks.
  const distance = new Map(roots.map((root) => [root, 0]));
  const parent = new Map<number, number>();
  for (const number of [...order].reverse()) {
    const reach = (distance.get(number) as number) + 1;
    for (const blocker of climb.get(number) as number[]) {
      const best = distance.get(blocker);
      const nearer = best === undefined || reach > best;
      const tied = reach === best && number < (parent.get(blocker) as number);
      if (!isRoot.has(blocker) && (nearer || tied)) {
        distance.set(blocker, reach);
        parent.set(blocker, number);
      }
    }
  }
  const children = new Map<number, number[]>();
  for (const [blocker, number] of parent) {
    children.set(number, [...(children.get(number) ?? []), blocker]);
  }

  const drawn = new Set(order);
  return drawTrees(roots, children, depth, (number) => {
    const under = new Set(children.get(number) ?? []);
    const others = ticketOf(number).openBlockers.filter((blocker) => !under.has(blocker));
    return lineOf(ticketOf(number), depth(number), others, drawn);
  });
}
