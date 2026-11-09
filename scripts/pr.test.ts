import { describe, expect, it } from "vitest";

import { branchRefusal, byVersion, failureSummary, shipArgs } from "./pr.ts";

describe("shipArgs", () => {
  it.each([
    [[], { check: true, bodyFile: undefined }],
    [["--no-check"], { check: false, bodyFile: undefined }],
    [["body.md"], { check: true, bodyFile: "body.md" }],
    [["body.md", "--no-check"], { check: false, bodyFile: "body.md" }],
  ])("reads %j", (args, expected) => {
    expect(shipArgs(args)).toEqual(expected);
  });

  it.each([[["--nocheck"]], [["--draft", "body.md"]], [["a.md", "b.md"]]])("refuses %j", (args) => {
    expect(shipArgs(args)).toBeUndefined();
  });
});

describe("branchRefusal", () => {
  it.each([
    ["", "not main"],
    ["main", "not main"],
    ["release/0.2", "ask the user"],
  ])("refuses %j", (branch, words) => {
    expect(branchRefusal(branch)).toContain(words);
  });

  it("lets a PR's branch push", () => {
    expect(branchRefusal("fewer-steps-per-pr")).toBeUndefined();
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

describe("byVersion", () => {
  it("puts the earliest version first, a short one as if padded, and other titles last", () => {
    const titles = ["Backlog", "1.0.0", "0.10.0", "0.4.1", "0.2.0", "0.4", "Someday"];
    expect(titles.sort(byVersion)).toEqual([
      "0.2.0",
      "0.4",
      "0.4.1",
      "0.10.0",
      "1.0.0",
      "Backlog",
      "Someday",
    ]);
  });
});
