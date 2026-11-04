import { expect, it } from "vitest";
import type { WorkbenchRow } from "$lib/state/workbench.svelte";
import { details, headline } from "./workbench-row";

const row: WorkbenchRow = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "johng",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  reasons: ["review_request"],
};

it("starts with the PR's number and title", () => {
  expect(headline(row)).toBe("#160662 gh-112564: Fix UAF for gc debug print");
});

it("says why the PR is here, who wrote it, where it goes and how CI went", () => {
  expect(details(row)).toBe("Review request · johng · into main · CI passed");
});

it("lists every reason", () => {
  expect(details({ ...row, reasons: ["review_request", "own_pr", "reviewed"] })).toBe(
    "Review request · Own PR · Reviewed · johng · into main · CI passed",
  );
});

it("says when a PR is a draft", () => {
  expect(details({ ...row, is_draft: true })).toBe("Review request · johng · into main · Draft · CI passed");
});

it("leaves CI out of a PR that has none", () => {
  expect(details({ ...row, ci: null })).toBe("Review request · johng · into main");
});

it("names a PR whose author's account has gone", () => {
  expect(details({ ...row, author: null })).toBe("Review request · deleted user · into main · CI passed");
});

it.each([
  ["EXPECTED", "CI expected"],
  ["ERROR", "CI error"],
  ["FAILURE", "CI failed"],
  ["PENDING", "CI running"],
] as const)("says what %s means", (ci, words) => {
  expect(details({ ...row, ci })).toBe(`Review request · johng · into main · ${words}`);
});

it("sends each PR into its own branch", () => {
  expect(details({ ...row, target_branch: "3.14" })).toBe("Review request · johng · into 3.14 · CI passed");
});
