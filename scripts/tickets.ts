// The order `just next` prints a milestone's tickets in. A ticket prints after every open blocker it has in
// the milestone, two spaces deeper than the one printed last, so a chain reads as a tree. Among the tickets
// at one place in that tree, the ones ready for an agent come first, and the ones with an open blocker
// outside the milestone come last, since no session can start them yet.

/** An open ticket of the milestone. */
export interface Ticket {
  number: number;
  title: string;
  labels: string[];
  /** The numbers of its open blockers, in the milestone or outside it. */
  openBlockers: number[];
}

const READY = "ready-for-agent";

/** The lines `just next` prints for the open tickets of a milestone, given in the order GitHub lists them. */
export function ticketLines(tickets: Ticket[]): string[] {
  const inMilestone = new Set(tickets.map((ticket) => ticket.number));
  const dependents = new Map<number, Ticket[]>();
  for (const ticket of tickets) {
    for (const number of ticket.openBlockers.filter((number) => inMilestone.has(number))) {
      dependents.set(number, [...(dependents.get(number) ?? []), ticket]);
    }
  }
  const printed = new Set<number>();
  const lines: string[] = [];

  const blockedOutside = (ticket: Ticket): boolean =>
    ticket.openBlockers.some((number) => !inMilestone.has(number));
  const rank = (ticket: Ticket): number =>
    (blockedOutside(ticket) ? 2 : 0) + (ticket.labels.includes(READY) ? 0 : 1);
  // `sort` keeps the order GitHub gave among tickets of the same rank.
  const inOrder = (group: Ticket[]): Ticket[] => [...group].sort((a, b) => rank(a) - rank(b));

  const line = (ticket: Ticket, depth: number): string => {
    // Blockers outside the milestone, and inside ones only when a cycle kept them from printing first.
    const pending = ticket.openBlockers.filter((number) => !printed.has(number));
    const blockedBy =
      pending.length === 0 ? "" : ` blocked by ${pending.map((n) => `#${String(n)}`).join(", ")}`;
    const indent = "  ".repeat(depth);
    return `${indent}#${String(ticket.number)} ${ticket.title} [${ticket.labels.join(", ")}]${blockedBy}`;
  };
  const print = (ticket: Ticket, depth: number): void => {
    lines.push(line(ticket, depth));
    printed.add(ticket.number);
    // A ticket whose last unprinted blocker in the milestone was this one goes under it.
    const unblocked = (dependents.get(ticket.number) ?? []).filter(
      (other) =>
        !printed.has(other.number) &&
        other.openBlockers.every((number) => printed.has(number) || !inMilestone.has(number)),
    );
    for (const other of inOrder(unblocked)) {
      print(other, depth + 1);
    }
  };

  const roots = tickets.filter((ticket) => !ticket.openBlockers.some((number) => inMilestone.has(number)));
  for (const ticket of inOrder(roots)) {
    print(ticket, 0);
  }
  // GitHub may not allow a cycle of blockers, but if one shows up, print its first ticket at the top and
  // the rest of the cycle under it.
  for (const ticket of tickets) {
    if (!printed.has(ticket.number)) {
      print(ticket, 0);
    }
  }
  return lines;
}
