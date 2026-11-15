import { describe, expect, it } from "vitest";

import { type TreeTicket, ticketsFromPages, treeLines } from "./tree.ts";

function ticket(number: number, openBlockers: number[] = [], more: Partial<TreeTicket> = {}): TreeTicket {
  const title = `Ticket ${String(number)}`;
  return { number, title, milestone: undefined, openBlockers, blockersElsewhere: [], ...more };
}

describe("treeLines", () => {
  it("draws a chain, with each ticket's depth and the greatest depth last", () => {
    expect(treeLines([ticket(3, [2]), ticket(2, [1]), ticket(1, [], { milestone: "0.3.3" })], [1])).toEqual([
      "#1 [0.3.3] Ticket 1 (1)",
      "└─ #2 Ticket 2 (2)",
      "   └─ #3 Ticket 3 (3)",
      "max depth: 3",
    ]);
  });

  it("prints a ticket with two blockers once, under the one on its longest chain", () => {
    // 4 is blocked by 2 (depth 2) and 3 (depth 3), so it goes under 3 and names 2.
    expect(treeLines([ticket(1), ticket(2, [1]), ticket(3, [2]), ticket(4, [2, 3])], [1])).toEqual([
      "#1 Ticket 1 (1)",
      "└─ #2 Ticket 2 (2)",
      "   └─ #3 Ticket 3 (3)",
      "      └─ #4 Ticket 4 (4)  [also #2]",
      "max depth: 4",
    ]);
  });

  it("puts a ticket under the lower number of two blockers as deep as each other", () => {
    expect(treeLines([ticket(1), ticket(2, [1]), ticket(3, [1]), ticket(4, [3, 2])], [1])).toEqual([
      "#1 Ticket 1 (1)",
      "├─ #2 Ticket 2 (2)",
      "│  └─ #4 Ticket 4 (3)  [also #3]",
      "└─ #3 Ticket 3 (2)",
      "max depth: 3",
    ]);
  });

  it("stars a blocker outside the trees and counts the depth it adds", () => {
    // 3 is blocked by 1, and by 10 and 8 outside the tree of 1. 10 sits at depth 3.
    const tickets = [ticket(1), ticket(3, [1, 10, 8]), ticket(10, [9]), ticket(9, [8]), ticket(8)];
    expect(treeLines(tickets, [1])).toEqual([
      "#1 Ticket 1 (1)",
      "└─ #3 Ticket 3 (4)  [also #8*, #10*]",
      "max depth: 4",
    ]);
  });

  it("counts a blocker GitHub didn't list as depth 1", () => {
    expect(treeLines([ticket(1), ticket(2, [1, 50])], [1])).toEqual([
      "#1 Ticket 1 (1)",
      "└─ #2 Ticket 2 (2)  [also #50*]",
      "max depth: 2",
    ]);
  });

  it("orders children by depth, then number", () => {
    // 2 sits at depth 3, through 9 outside the tree; 3, 4 and 5 at depth 2.
    const tickets = [ticket(1), ticket(5, [1]), ticket(2, [1, 9]), ticket(9, [8]), ticket(8)];
    tickets.push(ticket(4, [1]), ticket(3, [1]));
    expect(treeLines(tickets, [1])).toEqual([
      "#1 Ticket 1 (1)",
      "├─ #3 Ticket 3 (2)",
      "├─ #4 Ticket 4 (2)",
      "├─ #5 Ticket 5 (2)",
      "└─ #2 Ticket 2 (3)  [also #9*]",
      "max depth: 3",
    ]);
  });

  it("prints one tree per ticket named, and a ticket below two of them once", () => {
    const tickets = [ticket(1), ticket(2), ticket(3, [1, 2]), ticket(4, [3])];
    expect(treeLines(tickets, [2, 1, 2])).toEqual([
      "#2 Ticket 2 (1)",
      "#1 Ticket 1 (1)",
      "└─ #3 Ticket 3 (2)  [also #2]",
      "   └─ #4 Ticket 4 (3)",
      "max depth: 3",
    ]);
  });

  it("prints a ticket named as its own tree even when it's below another one named", () => {
    expect(treeLines([ticket(1), ticket(2, [1]), ticket(3, [2])], [1, 2])).toEqual([
      "#1 Ticket 1 (1)",
      "#2 Ticket 2 (2)  [also #1]",
      "└─ #3 Ticket 3 (3)",
      "max depth: 3",
    ]);
  });

  it("prints every open ticket when none is named, from each with no open blocker", () => {
    const tickets = [ticket(4, [3]), ticket(3), ticket(1), ticket(2, [1])];
    expect(treeLines(tickets, [])).toEqual([
      "#1 Ticket 1 (1)",
      "└─ #2 Ticket 2 (2)",
      "#3 Ticket 3 (1)",
      "└─ #4 Ticket 4 (2)",
      "max depth: 2",
    ]);
  });

  it("prints a ticket whose only blocker GitHub didn't list as a tree of its own", () => {
    expect(treeLines([ticket(2, [50])], [])).toEqual(["#2 Ticket 2 (2)  [also #50*]", "max depth: 2"]);
  });

  it("stars a blocker in another repository, counts it 1, and doesn't take it for the issue here", () => {
    expect(treeLines([ticket(5), ticket(2, [], { blockersElsewhere: ["other/repo#5"] })], [])).toEqual([
      "#2 Ticket 2 (2)  [also other/repo#5*]",
      "#5 Ticket 5 (1)",
      "max depth: 2",
    ]);
  });

  it("drops the blocker that closes a cycle, walking from the lowest number, and still names it", () => {
    expect(treeLines([ticket(2, [1]), ticket(1, [2]), ticket(3)], [])).toEqual([
      "#2 Ticket 2 (1)  [also #1]",
      "└─ #1 Ticket 1 (2)",
      "#3 Ticket 3 (1)",
      "max depth: 2",
    ]);
  });

  it("keeps a ticket hanging off a cycle under the cycle", () => {
    // 5 and 6 block each other, and 7 hangs off 6, and 4 off 7.
    expect(treeLines([ticket(5, [6]), ticket(6, [5]), ticket(7, [6]), ticket(4, [7])], [])).toEqual([
      "#5 Ticket 5 (1)  [also #6]",
      "└─ #6 Ticket 6 (2)",
      "   └─ #7 Ticket 7 (3)",
      "      └─ #4 Ticket 4 (4)",
      "max depth: 4",
    ]);
  });

  it("prints nothing when no ticket is open", () => {
    expect(treeLines([], [])).toEqual([]);
  });
});

describe("ticketsFromPages", () => {
  const page = (...nodes: unknown[]): unknown => ({
    data: { repository: { nameWithOwner: "owner/repo", issues: { nodes } } },
  });
  const blocker = (number: number, state: string, repository = "owner/repo"): unknown => ({
    number,
    state,
    repository: { nameWithOwner: repository },
  });

  it("reads every page, with each issue's milestone and only its open blockers", () => {
    const reply = JSON.stringify([
      page({
        number: 1,
        title: "First",
        milestone: { title: "0.3.3" },
        blockedBy: {
          totalCount: 3,
          nodes: [blocker(7, "OPEN"), blocker(8, "CLOSED"), blocker(9, "OPEN", "a/b")],
        },
      }),
      page({ number: 2, title: "Second", milestone: null, blockedBy: { totalCount: 0, nodes: [] } }),
    ]);
    expect(ticketsFromPages(reply)).toEqual({
      tickets: [
        { number: 1, title: "First", milestone: "0.3.3", openBlockers: [7], blockersElsewhere: ["a/b#9"] },
        { number: 2, title: "Second", milestone: undefined, openBlockers: [], blockersElsewhere: [] },
      ],
      warning: undefined,
    });
  });

  it("warns when GitHub listed only some of an issue's blockers", () => {
    const reply = JSON.stringify([
      page({ number: 1, title: "First", milestone: null, blockedBy: { totalCount: 101, nodes: [] } }),
    ]);
    expect(ticketsFromPages(reply).warning).toBe(
      "GitHub listed only the first 100 blockers of #1, so open ones may be missing.",
    );
  });
});
