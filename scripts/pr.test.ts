import { describe, expect, it } from "vitest";

import { type Pr, branchRefusal, failureSummary, shipArgs, unmergedReason } from "./pr.ts";

describe("shipArgs", () => {
  it.each([
    [[], { check: false, release: false, bodyFile: undefined }],
    [["--check"], { check: true, release: false, bodyFile: undefined }],
    [["body.md"], { check: false, release: false, bodyFile: "body.md" }],
    [["body.md", "--check"], { check: true, release: false, bodyFile: "body.md" }],
    [["--release"], { check: false, release: true, bodyFile: undefined }],
    [["--release", "--check", "body.md"], { check: true, release: true, bodyFile: "body.md" }],
  ])("reads %j", (args, expected) => {
    expect(shipArgs(args)).toEqual(expected);
  });

  it.each([[["--no-check"]], [["--draft", "body.md"]], [["a.md", "b.md"]]])("refuses %j", (args) => {
    expect(shipArgs(args)).toBeUndefined();
  });
});

describe("branchRefusal", () => {
  it.each([
    ["", false, "not main"],
    ["main", false, "not main"],
    ["main", true, "not main"],
    ["release/0.2.4", false, "--release"],
    ["fewer-steps-per-pr", true, "only a release/ branch"],
  ])("refuses %j with release %j", (branch, release, words) => {
    expect(branchRefusal(branch, release)).toContain(words);
  });

  it("lets a PR's branch push, and a release branch with --release", () => {
    expect(branchRefusal("fewer-steps-per-pr", false)).toBeUndefined();
    expect(branchRefusal("release/0.2.4", true)).toBeUndefined();
  });
});

describe("failureSummary", () => {
  it("keeps the last 30 lines, then the recipes that failed", () => {
    const lines = Array.from({ length: 40 }, (_, i) => `line ${String(i)}`);
    lines[5] = "error: recipe `lint` failed on line 59 with exit code 1";
    const summary = failureSummary(`${lines.join("\r\n")}\n`).split("\n");
    expect(summary.slice(0, 30)).toEqual(lines.slice(10));
    expect(summary.slice(30)).toEqual(["", "error: recipe `lint` failed on line 59 with exit code 1"]);
  });
});

describe("unmergedReason", () => {
  const merged: Pr = { number: 750, state: "MERGED", headRefName: "738-x", headRefOid: "abcdef0123" };
  const never = () => {
    throw new Error("asked git");
  };

  it("counts a branch its merged PR ends at, without asking git", () => {
    expect(unmergedReason("738-x", "abcdef0123", merged, never)).toBeUndefined();
  });

  it("counts a branch whose last commit the PR's holds", () => {
    const asked: string[][] = [];
    const isAncestor = (commit: string, of: string) => {
      asked.push([commit, of]);
      return true;
    };
    expect(unmergedReason("738-x", "1234567890", merged, isAncestor)).toBeUndefined();
    expect(asked).toEqual([["1234567890", "abcdef0123"]]);
  });

  it("refuses another branch's PR", () => {
    expect(unmergedReason("738-x", "abcdef0123", { ...merged, headRefName: "other" }, never)).toBe(
      "gh found #750, whose branch is other.",
    );
  });

  it("refuses a PR that hasn't merged", () => {
    expect(unmergedReason("738-x", "abcdef0123", { ...merged, state: "OPEN" }, never)).toBe(
      "#750 is open, not merged.",
    );
  });

  it("refuses a commit the PR never got", () => {
    expect(unmergedReason("738-x", "1234567890", merged, () => false)).toBe(
      "738-x is at 1234567, which #750 doesn't hold.",
    );
  });
});
