import { expect, it } from "vitest";
import type { IssueItem, WorkbenchRow } from "$lib/state/workbench.svelte";
import { ago, details, headline, issueDetails, standaloneDetails } from "./workbench-row";

const row: WorkbenchRow = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "some-author",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: ["review_request"],
};

const issue: IssueItem = {
  number: 112564,
  title: "GIL drop during collection",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [row, { ...row, number: 160663 }],
};

const threeHoursLater = new Date("2026-11-04T13:00:00Z");

it("starts with the PR's number and title", () => {
  expect(headline(row)).toBe("#160662 gh-112564: Fix UAF for gc debug print");
});

it("says why the PR is here, who wrote it, where it goes and how CI went", () => {
  expect(details(row)).toBe("Review request · some-author · into main · CI passed");
});

it("lists every reason", () => {
  expect(details({ ...row, reasons: ["review_request", "own_pr", "reviewed"] })).toBe(
    "Review request · Own PR · Reviewed · some-author · into main · CI passed",
  );
});

it("says when a PR is a draft", () => {
  expect(details({ ...row, is_draft: true })).toBe(
    "Review request · some-author · into main · Draft · CI passed",
  );
});

it("leaves CI out of a PR that has none", () => {
  expect(details({ ...row, ci: null })).toBe("Review request · some-author · into main");
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
  expect(details({ ...row, ci })).toBe(`Review request · some-author · into main · ${words}`);
});

it("sends each PR into its own branch", () => {
  expect(details({ ...row, target_branch: "3.14" })).toBe(
    "Review request · some-author · into 3.14 · CI passed",
  );
});

it.each([
  ["2026-11-04T10:00:30Z", "just now"],
  ["2026-11-04T10:01:00Z", "1m ago"],
  ["2026-11-04T10:59:59Z", "59m ago"],
  ["2026-11-04T11:00:00Z", "1h ago"],
  ["2026-11-05T09:59:59Z", "23h ago"],
  ["2026-11-05T10:00:00Z", "1d ago"],
  ["2026-11-14T10:00:00Z", "10d ago"],
])("at %s says %s", (now, words) => {
  expect(ago("2026-11-04T10:00:00Z", new Date(now))).toBe(words);
});

it("reads a time ahead of this computer's clock as now", () => {
  expect(ago("2026-11-04T10:00:00Z", new Date("2026-11-04T09:00:00Z"))).toBe("just now");
});

it("starts an Issue with its number and title", () => {
  expect(headline(issue)).toBe("#112564 GIL drop during collection");
});

it("says who an Issue involves, how many PRs fix it and when it changed", () => {
  expect(issueDetails(issue, threeHoursLater)).toBe("issue-author, some-author · 2 PRs · 3h ago");
});

it("counts one PR in the singular", () => {
  expect(issueDetails({ ...issue, pull_requests: [row] }, threeHoursLater)).toBe(
    "issue-author, some-author · 1 PR · 3h ago",
  );
});

it("leaves people out of an Issue whose people have all gone", () => {
  expect(issueDetails({ ...issue, people: [] }, threeHoursLater)).toBe("2 PRs · 3h ago");
});

it("says when a Standalone PR changed", () => {
  expect(standaloneDetails(row, threeHoursLater)).toBe(
    "Review request · some-author · into main · CI passed · 3h ago",
  );
});
