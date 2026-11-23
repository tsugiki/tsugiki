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

/** What `walk` keeps of a graph. */
interface Walked {
  /** Each ticket the walk reached, with its `next` tickets in order of number, less those closing a cycle. */
  edges: Map<number, number[]>;
  /** Each ticket the walk reached, after every ticket its `edges` name. */
  order: number[];
}

/**
 * A walk from each of `starts` in turn along `next`, through every ticket it can reach. GitHub's docs don't
 * say it refuses a cycle of blockers, so the walk drops an edge to a ticket still on its path, which would
 * close one, and what it keeps holds no cycle.
 */
function walk(starts: number[], next: (number: number) => number[]): Walked {
  const edges = new Map<number, number[]>();
  const order: number[] = [];
  const walking = new Set<number>();
  const visit = (number: number): void => {
    walking.add(number);
    const kept = next(number)
      .filter((other) => !walking.has(other))
      .sort((a, b) => a - b);
    edges.set(number, kept);
    for (const other of kept) {
      if (!edges.has(other)) {
        visit(other);
      }
    }
    walking.delete(number);
    order.push(number);
  };
  for (const start of starts) {
    if (!edges.has(start)) {
      visit(start);
    }
  }
  return { edges, order };
}

/**
 * Each ticket's depth from a walk `up` its open blockers: 1 plus the depth of its deepest blocker the walk
 * kept. A blocker in another repository, or one GitHub didn't list as an open issue here, has no blockers
 * we know of, so it counts 1.
 */
function depthsOf(up: Walked, byNumber: Map<number, TreeTicket>): (number: number) => number {
  const depths = new Map<number, number>();
  for (const number of up.order) {
    const below = (up.edges.get(number) as number[]).map((blocker) => depths.get(blocker) as number);
    const outside = blockedOutside(byNumber.get(number) as TreeTicket, byNumber) ? 1 : 0;
    depths.set(number, 1 + Math.max(outside, ...below));
  }
  return (number) => depths.get(number) as number;
}

/** The open blockers of `number` that are open issues here. */
function blockersHere(byNumber: Map<number, TreeTicket>, number: number): number[] {
  return (byNumber.get(number) as TreeTicket).openBlockers.filter((blocker) => byNumber.has(blocker));
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
): Drawn {
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
  return { lines, deepest };
}

/** The lines of some trees, and the greatest depth any of them reaches. */
interface Drawn {
  lines: string[];
  deepest: number;
}

/** The lines of the trees `drawn`, with the greatest depth last, or none when no tree was drawn. */
function withDepth(drawn: Drawn[]): string[] {
  const lines = drawn.flatMap((trees) => trees.lines);
  const deepest = Math.max(0, ...drawn.map((trees) => trees.deepest));
  return lines.length === 0 ? [] : [...lines, `max depth: ${String(deepest)}`];
}

/**
 * The lines `just tree` prints for the open `tickets`: one tree for each ticket `named`, in that order, or
 * with none named, one for each ticket with no open blocker. Each ticket named must be in `tickets`.
 */
export function treeLines(tickets: TreeTicket[], named: number[]): string[] {
  const byNumber = new Map(tickets.map((ticket) => [ticket.number, ticket]));

  // Each ticket's depth comes from a walk up its open blockers from the lowest number, in the trees or not.
  // With none named, a tree grows from each ticket that walk kept no blocker for: one with no open blocker,
  // or the first ticket of a cycle nothing else blocks.
  const up = walk(
    tickets.map((ticket) => ticket.number).sort((a, b) => a - b),
    (number) => blockersHere(byNumber, number),
  );
  const depth = depthsOf(up, byNumber);
  const roots =
    named.length > 0
      ? [...new Set(named)]
      : [...up.edges]
          .filter(([, blockers]) => blockers.length === 0)
          .map(([number]) => number)
          .sort((a, b) => a - b);

  // A walk down from the roots reaches each ticket they block, through a cycle too. The walk up can drop
  // the edge that leads into the trees, as it starts from the lowest number wherever the trees start.
  const dependents = new Map<number, number[]>();
  for (const ticket of tickets) {
    for (const blocker of blockersHere(byNumber, ticket.number)) {
      dependents.set(blocker, [...(dependents.get(blocker) ?? []), ticket.number]);
    }
  }
  const down = walk(roots, (number) => dependents.get(number) ?? []);
  const isRoot = new Set(roots);
  const inTrees = new Set(down.order);

  // Each ticket below a root goes under its deepest blocker the walk down kept, the lowest number on a tie.
  // Those edges hold no cycle, so no ticket hangs from its own child.
  const parent = new Map<number, number>();
  for (const [number, below] of down.edges) {
    for (const child of below.filter((ticket) => !isRoot.has(ticket))) {
      const best = parent.get(child);
      const deeper = best === undefined || depth(number) > depth(best);
      if (deeper || (depth(number) === depth(best) && number < best)) {
        parent.set(child, number);
      }
    }
  }
  const children = new Map<number, number[]>();
  for (const [child, number] of parent) {
    children.set(number, [...(children.get(number) ?? []), child]);
  }

  const drawn = drawTrees(roots, children, depth, (number, above) => {
    const ticket = byNumber.get(number) as TreeTicket;
    const others = ticket.openBlockers.filter((blocker) => blocker !== above);
    return lineOf(ticket, depth(number), others, inTrees);
  });
  return withDepth([drawn]);
}

/**
 * The lines `just tree --up` prints for the open `tickets`: one tree for each ticket `named`, in that order,
 * with each ticket's open blockers under it. Each tree stands alone, so a blocker of two tickets named
 * prints in each tree. Each ticket named must be in `tickets`.
 */
export function blockerLines(tickets: TreeTicket[], named: number[]): string[] {
  const byNumber = new Map(tickets.map((ticket) => [ticket.number, ticket]));
  return withDepth([...new Set(named)].map((root) => blockerTree(byNumber, root)));
}

/** The tree of the open blockers above `root`. */
function blockerTree(byNumber: Map<number, TreeTicket>, root: number): Drawn {
  const ticketOf = (number: number): TreeTicket => byNumber.get(number) as TreeTicket;

  // A walk up from `root` reaches each open blocker here, and `[also]` names a blocker it dropped. Each
  // ticket's depth comes from this walk, so inside a cycle it can differ from the depth `just tree` prints,
  // whose walk starts from the lowest number.
  const up = walk([root], (number) => blockersHere(byNumber, number));
  const depth = depthsOf(up, byNumber);

  // Each blocker goes under the ticket it blocks that sits furthest from `root`, the lowest number on a tie,
  // so the tree shows its longest chain. `up.order` reversed reaches each blocker after every ticket it
  // blocks, and `root` stays on the walk's path throughout, so `up.edges` never names it.
  const distance = new Map([[root, 0]]);
  const parent = new Map<number, number>();
  for (const number of [...up.order].reverse()) {
    const reach = (distance.get(number) as number) + 1;
    for (const blocker of up.edges.get(number) as number[]) {
      const best = distance.get(blocker);
      const nearer = best === undefined || reach > best;
      const tied = reach === best && number < (parent.get(blocker) as number);
      if (nearer || tied) {
        distance.set(blocker, reach);
        parent.set(blocker, number);
      }
    }
  }
  const children = new Map<number, number[]>();
  for (const [blocker, number] of parent) {
    children.set(number, [...(children.get(number) ?? []), blocker]);
  }

  const drawn = new Set(up.order);
  return drawTrees([root], children, depth, (number) => {
    const under = new Set(children.get(number) ?? []);
    const others = ticketOf(number).openBlockers.filter((blocker) => !under.has(blocker));
    return lineOf(ticketOf(number), depth(number), others, drawn);
  });
}
