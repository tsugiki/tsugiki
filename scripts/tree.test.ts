import { describe, expect, it } from "vitest";

import { blockerLines, type TreeTicket, ticketsFromPages, treeLines } from "./tree.ts";

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

  it("grows a cycle nothing else blocks from its lowest number, and names the blocker it drops", () => {
    expect(treeLines([ticket(2, [1]), ticket(1, [2]), ticket(3)], [])).toEqual([
      "#1 Ticket 1 (1)  [also #2]",
      "└─ #2 Ticket 2 (2)",
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

  it("draws each ticket the one named blocks through a cycle, whichever ticket is named", () => {
    // 9 is blocked by 1; 1 and 2 block each other, and 5 blocks 1. 5 blocks 2 through 1, and 2 blocks 1.
    const tickets = [ticket(9, [1]), ticket(1, [2, 5]), ticket(2, [1]), ticket(5)];
    expect(treeLines(tickets, [5])).toEqual([
      "#5 Ticket 5 (1)",
      "└─ #1 Ticket 1 (2)  [also #2]",
      "   ├─ #2 Ticket 2 (3)",
      "   └─ #9 Ticket 9 (3)",
      "max depth: 3",
    ]);
    expect(treeLines(tickets, [2])).toEqual([
      "#2 Ticket 2 (1)  [also #1]",
      "└─ #1 Ticket 1 (2)  [also #5*]",
      "   └─ #9 Ticket 9 (3)",
      "max depth: 3",
    ]);
  });

  it("hangs a ticket in a cycle under its deepest kept blocker, never under its own child", () => {
    // 1 and 2 block each other, 5 blocks 1, and 3 blocks 2 from outside the tree. 1 goes under 5: under 2
    // it would hang from its own child.
    const tickets = [ticket(1, [2, 5]), ticket(2, [1, 3]), ticket(3), ticket(5)];
    expect(treeLines(tickets, [5])).toEqual([
      "#5 Ticket 5 (1)",
      "└─ #1 Ticket 1 (2)  [also #2]",
      "   └─ #2 Ticket 2 (3)  [also #3*]",
      "max depth: 3",
    ]);
  });

  it("counts the depth of a chain through a cycle along the chain it draws", () => {
    // 7 blocks 3; 3, 1 and 2 block each other in a ring, and 2 blocks 8.
    const tickets = [ticket(3, [2, 7]), ticket(1, [3]), ticket(2, [1]), ticket(8, [2]), ticket(7)];
    expect(treeLines(tickets, [7])).toEqual([
      "#7 Ticket 7 (1)",
      "└─ #3 Ticket 3 (2)  [also #2]",
      "   └─ #1 Ticket 1 (3)",
      "      └─ #2 Ticket 2 (4)",
      "         └─ #8 Ticket 8 (5)",
      "max depth: 5",
    ]);
  });

  it("grows a cycle a ticket outside it blocks from that ticket, with none named", () => {
    // 1 and 2 block each other, and 5 blocks 1.
    expect(treeLines([ticket(1, [2, 5]), ticket(2, [1]), ticket(5)], [])).toEqual([
      "#5 Ticket 5 (1)",
      "└─ #1 Ticket 1 (2)  [also #2]",
      "   └─ #2 Ticket 2 (3)",
      "max depth: 3",
    ]);
  });

  it("prints nothing when no ticket is open", () => {
    expect(treeLines([], [])).toEqual([]);
  });
});

describe("blockerLines", () => {
  it("draws a chain of blockers upward, each ticket's open blockers under it", () => {
    const tickets = [ticket(1, [], { milestone: "0.3.3" }), ticket(2, [1]), ticket(3, [2])];
    expect(blockerLines(tickets, [3])).toEqual([
      "#3 Ticket 3 (3)",
      "└─ #2 Ticket 2 (2)",
      "   └─ #1 [0.3.3] Ticket 1 (1)",
      "max depth: 3",
    ]);
  });

  it("prints a blocker of two tickets once, on the longest chain from the ticket named", () => {
    // 2 blocks 4 and 3, and 3 blocks 4, so 2 goes under 3 and 4 names it.
    expect(blockerLines([ticket(1), ticket(2, [1]), ticket(3, [2]), ticket(4, [2, 3])], [4])).toEqual([
      "#4 Ticket 4 (4)  [also #2]",
      "└─ #3 Ticket 3 (3)",
      "   └─ #2 Ticket 2 (2)",
      "      └─ #1 Ticket 1 (1)",
      "max depth: 4",
    ]);
  });

  it("puts a blocker under the lower number of two tickets as far from the one named", () => {
    expect(blockerLines([ticket(1), ticket(2, [1]), ticket(3, [1]), ticket(4, [3, 2])], [4])).toEqual([
      "#4 Ticket 4 (3)",
      "├─ #2 Ticket 2 (2)",
      "│  └─ #1 Ticket 1 (1)",
      "└─ #3 Ticket 3 (2)  [also #1]",
      "max depth: 3",
    ]);
  });

  it("orders a ticket's blockers by depth, then number", () => {
    const tickets = [ticket(5, [2, 4, 1]), ticket(2, [3]), ticket(3), ticket(4), ticket(1)];
    expect(blockerLines(tickets, [5])).toEqual([
      "#5 Ticket 5 (3)",
      "├─ #1 Ticket 1 (1)",
      "├─ #4 Ticket 4 (1)",
      "└─ #2 Ticket 2 (2)",
      "   └─ #3 Ticket 3 (1)",
      "max depth: 3",
    ]);
  });

  it("prints a ticket with no open blocker alone", () => {
    expect(blockerLines([ticket(1), ticket(2, [1])], [1])).toEqual(["#1 Ticket 1 (1)", "max depth: 1"]);
  });

  it("stars a blocker GitHub didn't list and one in another repository", () => {
    const tickets = [ticket(2, [50], { blockersElsewhere: ["other/repo#3"] })];
    expect(blockerLines(tickets, [2])).toEqual([
      "#2 Ticket 2 (2)  [also #50*, other/repo#3*]",
      "max depth: 2",
    ]);
  });

  it("draws one tree per ticket named, each with all of its blockers, once per number named", () => {
    expect(blockerLines([ticket(1), ticket(2, [1])], [2, 1, 2])).toEqual([
      "#2 Ticket 2 (2)",
      "└─ #1 Ticket 1 (1)",
      "#1 Ticket 1 (1)",
      "max depth: 2",
    ]);
  });

  it("draws a blocker of two tickets named in each tree", () => {
    const tickets = [ticket(1, [2, 3]), ticket(2, [4]), ticket(3, [4]), ticket(4)];
    expect(blockerLines(tickets, [1, 2])).toEqual([
      "#1 Ticket 1 (3)",
      "├─ #2 Ticket 2 (2)",
      "│  └─ #4 Ticket 4 (1)",
      "└─ #3 Ticket 3 (2)  [also #4]",
      "#2 Ticket 2 (2)",
      "└─ #4 Ticket 4 (1)",
      "max depth: 3",
    ]);
  });

  it("walks a ticket in a cycle from itself, though another ticket named reached it first", () => {
    // 9 is blocked by 1; 1 and 2 block each other, and 5 blocks 1.
    const tickets = [ticket(9, [1]), ticket(1, [2, 5]), ticket(2, [1]), ticket(5)];
    expect(blockerLines(tickets, [9, 2])).toEqual([
      "#9 Ticket 9 (3)",
      "└─ #1 Ticket 1 (2)",
      "   ├─ #2 Ticket 2 (1)  [also #1]",
      "   └─ #5 Ticket 5 (1)",
      "#2 Ticket 2 (3)",
      "└─ #1 Ticket 1 (2)  [also #2]",
      "   └─ #5 Ticket 5 (1)",
      "max depth: 3",
    ]);
  });

  it("keeps a blocker on its longer chain when a shorter one reaches it later", () => {
    // 1 blocks 2, one step from 10, and 4, three steps away through 3; the walk meets 4's chain first.
    const tickets = [ticket(10, [2, 3]), ticket(3, [4]), ticket(4, [1]), ticket(2, [1]), ticket(1)];
    expect(blockerLines(tickets, [10])).toEqual([
      "#10 Ticket 10 (4)",
      "├─ #2 Ticket 2 (2)  [also #1]",
      "└─ #3 Ticket 3 (3)",
      "   └─ #4 Ticket 4 (2)",
      "      └─ #1 Ticket 1 (1)",
      "max depth: 4",
    ]);
  });

  it("prints nothing when no ticket is named", () => {
    expect(blockerLines([ticket(1)], [])).toEqual([]);
  });

  it("breaks a tie by number, though one of the two tickets runs deeper", () => {
    // 1 blocks 2 and 3, both one step from 10, and 3 runs deeper through 5 and 6.
    const tickets = [
      ticket(10, [2, 3]),
      ticket(2, [1]),
      ticket(3, [1, 5]),
      ticket(5, [6]),
      ticket(6),
      ticket(1),
    ];
    expect(blockerLines(tickets, [10])).toEqual([
      "#10 Ticket 10 (4)",
      "├─ #2 Ticket 2 (2)",
      "│  └─ #1 Ticket 1 (1)",
      "└─ #3 Ticket 3 (3)  [also #1]",
      "   └─ #5 Ticket 5 (2)",
      "      └─ #6 Ticket 6 (1)",
      "max depth: 4",
    ]);
  });

  it("walks up from the ticket named, dropping the blocker that closes a cycle and still naming it", () => {
    const tickets = [ticket(1, [2]), ticket(2, [1])];
    expect(blockerLines(tickets, [1])).toEqual([
      "#1 Ticket 1 (2)",
      "└─ #2 Ticket 2 (1)  [also #1]",
      "max depth: 2",
    ]);
    expect(blockerLines(tickets, [2])).toEqual([
      "#2 Ticket 2 (2)",
      "└─ #1 Ticket 1 (1)  [also #2]",
      "max depth: 2",
    ]);
  });

  it("keeps the blockers of a ticket in a cycle, whichever ticket of it is named", () => {
    // 1 and 2 block each other, and 5 blocks 1.
    expect(blockerLines([ticket(1, [2, 5]), ticket(2, [1]), ticket(5)], [2])).toEqual([
      "#2 Ticket 2 (3)",
      "└─ #1 Ticket 1 (2)  [also #2]",
      "   └─ #5 Ticket 5 (1)",
      "max depth: 3",
    ]);
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
