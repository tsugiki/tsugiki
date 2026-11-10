import { describe, expect, it } from "vitest";

import { type Ticket, ticketLines } from "./tickets.ts";

function ticket(number: number, labels: string[] = [], openBlockers: number[] = []): Ticket {
  return { number, title: `Ticket ${String(number)}`, labels, openBlockers };
}

const READY = ["ready-for-agent"];

describe("ticketLines", () => {
  it("prints the tickets ready for an agent first, each group in the order given", () => {
    expect(
      ticketLines([ticket(1, ["bug"]), ticket(2, READY), ticket(3), ticket(4, ["enhancement", ...READY])]),
    ).toEqual([
      "#2 Ticket 2 [ready-for-agent]",
      "#4 Ticket 4 [enhancement, ready-for-agent]",
      "#1 Ticket 1 [bug]",
      "#3 Ticket 3 []",
    ]);
  });

  it("prints a ticket under its blocker, two spaces deeper", () => {
    expect(ticketLines([ticket(274, READY, [273]), ticket(273, READY)])).toEqual([
      "#273 Ticket 273 [ready-for-agent]",
      "  #274 Ticket 274 [ready-for-agent]",
    ]);
  });

  it("indents a chain as a tree", () => {
    expect(ticketLines([ticket(3, READY, [2]), ticket(2, READY, [1]), ticket(1), ticket(4)])).toEqual([
      "#1 Ticket 1 []",
      "  #2 Ticket 2 [ready-for-agent]",
      "    #3 Ticket 3 [ready-for-agent]",
      "#4 Ticket 4 []",
    ]);
  });

  it("puts a ticket with two blockers under the one printed last", () => {
    expect(ticketLines([ticket(3, [], [1, 2]), ticket(1, READY), ticket(2)])).toEqual([
      "#1 Ticket 1 [ready-for-agent]",
      "#2 Ticket 2 []",
      "  #3 Ticket 3 []",
    ]);
  });

  it("waits for a blocker printed deeper in a sibling's subtree", () => {
    // 4 is blocked by 3 and 2. 3 prints under 1, before 2, so 4 goes under 2.
    expect(
      ticketLines([
        ticket(4, [], [3, 2]),
        ticket(3, [], [1]),
        ticket(2, [], [0]),
        ticket(1, READY, [0]),
        ticket(0),
      ]),
    ).toEqual([
      "#0 Ticket 0 []",
      "  #1 Ticket 1 [ready-for-agent]",
      "    #3 Ticket 3 []",
      "  #2 Ticket 2 []",
      "    #4 Ticket 4 []",
    ]);
  });

  it("keeps ready-for-agent first among the tickets under one blocker", () => {
    expect(ticketLines([ticket(2, [], [1]), ticket(3, READY, [1]), ticket(1)])).toEqual([
      "#1 Ticket 1 []",
      "  #3 Ticket 3 [ready-for-agent]",
      "  #2 Ticket 2 []",
    ]);
  });

  it("names an open blocker outside the milestone", () => {
    expect(ticketLines([ticket(2, READY, [1, 90, 91]), ticket(1)])).toEqual([
      "#1 Ticket 1 []",
      "  #2 Ticket 2 [ready-for-agent] blocked by #90, #91",
    ]);
  });

  it("puts a ticket blocked outside the milestone after the ones a session can start", () => {
    expect(
      ticketLines([ticket(5, READY, [90]), ticket(6, ["bug"]), ticket(7, READY), ticket(8, [], [91])]),
    ).toEqual([
      "#7 Ticket 7 [ready-for-agent]",
      "#6 Ticket 6 [bug]",
      "#5 Ticket 5 [ready-for-agent] blocked by #90",
      "#8 Ticket 8 [] blocked by #91",
    ]);
  });

  it("breaks a cycle at its first ticket and keeps the tree below it", () => {
    expect(ticketLines([ticket(1, [], [2]), ticket(2, [], [1]), ticket(3, [], [1]), ticket(4)])).toEqual([
      "#4 Ticket 4 []",
      "#1 Ticket 1 [] blocked by #2",
      "  #2 Ticket 2 []",
      "  #3 Ticket 3 []",
    ]);
  });
});
