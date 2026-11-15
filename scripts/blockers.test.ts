import { describe, expect, it } from "vitest";

import { type BlockedBy, cutWarning, openBlockers } from "./blockers.ts";

const HERE = "owner/repo";

function blockedBy(totalCount: number, ...nodes: [number, "OPEN" | "CLOSED", string?][]): BlockedBy {
  return {
    totalCount,
    nodes: nodes.map(([number, state, repository = HERE]) => ({
      number,
      state,
      repository: { nameWithOwner: repository },
    })),
  };
}

describe("openBlockers", () => {
  it("keeps the open blockers, and names the ones in other repositories in full", () => {
    const given = blockedBy(
      4,
      [7, "OPEN"],
      [8, "CLOSED"],
      [7, "OPEN", "other/repo"],
      [9, "CLOSED", "other/repo"],
    );
    expect(openBlockers(given, HERE)).toEqual({ openBlockers: [7], blockersElsewhere: ["other/repo#7"] });
  });
});

describe("cutWarning", () => {
  it("names each issue GitHub listed only some blockers of", () => {
    const issues = [
      { number: 1, blockedBy: blockedBy(101, [2, "OPEN"]) },
      { number: 3, blockedBy: blockedBy(1, [2, "OPEN"]) },
      { number: 4, blockedBy: blockedBy(150) },
    ];
    expect(cutWarning(issues)).toBe(
      "GitHub listed only the first 100 blockers of #1, #4, so open ones may be missing.",
    );
  });

  it("says nothing when GitHub listed every blocker", () => {
    expect(cutWarning([{ number: 1, blockedBy: blockedBy(1, [2, "OPEN"]) }])).toBeUndefined();
  });
});
