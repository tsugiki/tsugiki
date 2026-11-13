import { expect, it } from "vitest";
import type { CantCheck, IssueItem, WorkbenchRow, Why } from "$lib/state/workbench.svelte";
import {
  ago,
  backportWords,
  details,
  detailWords,
  headline,
  peopleOf,
  preview,
  prStateWords,
  summary,
  treeCount,
} from "./workbench-row";

const row: WorkbenchRow = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "some-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: ["review_request"],
  your_review: { kind: "unreviewed" },
  seen: { mark: { numbers: [160662], up_to: null }, state: "seen" },
};

const issue: IssueItem = {
  number: 112564,
  title: "GIL drop during collection",
  state: "OPEN",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [
    { kind: "pr", ...row, backports: [] },
    { kind: "pr", ...row, number: 160663, backports: [] },
  ],
  seen: {
    mark: { numbers: [112564, 160662, 160663], up_to: null },
    state: "seen",
  },
};

it("starts with the PR's number and title", () => {
  expect(headline(row)).toBe("#160662 gh-112564: Fix UAF for gc debug print");
});

it("says why the PR is here, who wrote it, where it goes and how CI went", () => {
  expect(details(row)).toBe("Review request · some-author · into main · Open · CI passed");
});

it("lists every reason", () => {
  expect(details({ ...row, reasons: ["review_request", "own_pr", "reviewed"] })).toBe(
    "Review request · Own PR · Reviewed · some-author · into main · Open · CI passed",
  );
});

it("says when a PR is a draft", () => {
  expect(details({ ...row, is_draft: true })).toBe(
    "Review request · some-author · into main · Open · Draft · CI passed",
  );
});

it("leaves CI out of a PR that has none", () => {
  expect(details({ ...row, ci: null })).toBe("Review request · some-author · into main · Open");
});

it("names a PR whose author's account has gone", () => {
  expect(details({ ...row, author: null })).toBe(
    "Review request · deleted user · into main · Open · CI passed",
  );
});

it.each([
  ["EXPECTED", "CI expected"],
  ["ERROR", "CI error"],
  ["FAILURE", "CI failed"],
  ["PENDING", "CI running"],
] as const)("says what %s means", (ci, words) => {
  expect(details({ ...row, ci })).toBe(`Review request · some-author · into main · Open · ${words}`);
});

it("sends each PR into its own branch", () => {
  expect(details({ ...row, target_branch: "3.14" })).toBe(
    "Review request · some-author · into 3.14 · Open · CI passed",
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

it("says what state a PR is in", () => {
  expect(details({ ...row, state: "MERGED" })).toContain("· Merged ·");
  expect(details({ ...row, state: "CLOSED" })).toContain("· Closed ·");
});

it("names a Backport's kind", () => {
  expect(details({ ...row, reasons: [], backport: "bot" })).toBe(
    "Bot backport · some-author · into main · Open · CI passed",
  );
  expect(details({ ...row, reasons: [], backport: "manual" })).toBe(
    "Manual backport · some-author · into main · Open · CI passed",
  );
});

it("counts each Backport and each PR in an Issue's tree", () => {
  const tree: IssueItem = {
    ...issue,
    pull_requests: [
      { kind: "pr", ...row, backports: [{ ...row, number: 2, backport: "bot", state: "MERGED" }] },
      { kind: "backport", ...row, number: 3, backport: "manual" },
    ],
  };
  expect(treeCount(tree)).toBe("3 PRs");
  expect(summary({ kind: "issue", ...tree, attention: null })).toBe("2 open of 3 PRs");
});

it("counts one PR in the singular", () => {
  expect(treeCount({ ...issue, pull_requests: [{ kind: "pr", ...row, backports: [] }] })).toBe("1 PR");
});

it("starts an Issue with its number and title", () => {
  expect(headline(issue)).toBe("#112564 GIL drop during collection");
});

it("names the people an Issue involves, or a PR's author", () => {
  expect(peopleOf({ kind: "issue", ...issue, attention: null })).toBe("issue-author, some-author");
  expect(peopleOf({ kind: "standalone_pr", ...row, backports: [], attention: null })).toBe("some-author");
  expect(
    peopleOf({ kind: "standalone_backport", ...row, author: null, backport: "bot", attention: null }),
  ).toBe("deleted user");
});

it("shows a dash for an Issue whose people have all gone", () => {
  expect(peopleOf({ kind: "issue", ...issue, people: [], attention: null })).toBe("—");
});

it("says an Issue has no PR yet, and gives a PR's facts", () => {
  expect(summary({ kind: "issue", ...issue, pull_requests: [], attention: null })).toBe("No PR yet");
  expect(summary({ kind: "standalone_pr", ...row, backports: [], attention: null })).toBe(details(row));
});

it.each<[Why, string]>([
  [{ kind: "review_requested" }, "Review requested from you"],
  [{ kind: "stale_review" }, "Changed since your Review"],
  [{ kind: "ci_failed", checks: [] }, "CI failed"],
  [{ kind: "ci_failed", checks: ["Tests / Ubuntu", "Docs"] }, "CI failed: Tests / Ubuntu, Docs"],
  [{ kind: "ci_passed" }, "CI passed"],
  [{ kind: "changes_requested", by: "some-reviewer" }, "Changes requested by some-reviewer"],
  [{ kind: "approved", by: "some-reviewer" }, "Approved by some-reviewer"],
  [{ kind: "reviewed", by: "some-reviewer" }, "Reviewed by some-reviewer"],
  [{ kind: "comment", by: "some-author" }, "New comment from some-author"],
  [{ kind: "commits", by: "some-author" }, "New commits from some-author"],
  [{ kind: "ready_for_review", by: "some-author" }, "Ready for review, from some-author"],
  [{ kind: "mentioned" }, "You were mentioned"],
  [{ kind: "backport_failed", target: "3.13" }, "Bot backport to 3.13 failed"],
  [{ kind: "marked_unseen" }, "Marked unseen"],
])("a preview says %j as %s", (why, words) => {
  expect(preview({ since: "2026-11-04T10:00:00Z", why: [why] })).toBe(words);
});

it("a preview lists every reason in the order it came", () => {
  const why: Why[] = [{ kind: "ci_passed" }, { kind: "mentioned" }];
  expect(preview({ since: "2026-11-04T10:00:00Z", why })).toBe("CI passed · You were mentioned");
});

it("says a PR's state, CI and where the user stands in the Issue view, each with its tone", () => {
  expect(prStateWords(row)).toEqual([
    { words: "Open", tone: "plain" },
    { words: "CI passed", tone: "good" },
    { words: "Review request", tone: "request" },
  ]);
  const words = (changes: Partial<WorkbenchRow>) => prStateWords({ ...row, ...changes }).map((w) => w.words);
  expect(words({ reasons: [], your_review: { kind: "reviewed" } })).toEqual([
    "Open",
    "CI passed",
    "Reviewed",
  ]);
  expect(words({ reasons: [], is_draft: true, ci: null })).toEqual(["Open", "Draft"]);
});

const stale: WorkbenchRow = {
  ...row,
  reasons: ["reviewed"],
  your_review: {
    kind: "stale_review",
    files: [
      { path: "Lib/ast.py", whitespace_only: false },
      { path: "Python/ast.c", whitespace_only: true },
    ],
  },
};

it("says a Stale review in the danger tone, and its hover lists the files changed since the Review", () => {
  expect(prStateWords(stale).at(-1)).toEqual({
    words: "Stale review",
    tone: "bad",
    hover: "Changed since your Review: 2 files\nLib/ast.py\nPython/ast.c (whitespace only)",
  });
});

it("counts one file changed since the Review as one", () => {
  const one: WorkbenchRow = {
    ...stale,
    your_review: { kind: "stale_review", files: [{ path: "Lib/ast.py", whitespace_only: false }] },
  };
  expect(prStateWords(one).at(-1)?.hover).toBe("Changed since your Review: 1 file\nLib/ast.py");
});

it("shows a Stale review on the Workbench row in place of the Reviewed reason", () => {
  const words = detailWords(stale);
  expect(words.map((word) => word.words)).toEqual([
    "Stale review",
    "some-author",
    "into main",
    "Open",
    "CI passed",
  ]);
  expect(words[0]?.hover).toContain("Python/ast.c (whitespace only)");
  expect(details(stale)).toBe("Stale review · some-author · into main · Open · CI passed");
});

it("shows a Stale review on the row of a PR that is no longer open, which has no reasons", () => {
  const merged = detailWords({ ...stale, reasons: [], state: "MERGED" });
  expect(merged.map((word) => word.words)).toEqual([
    "Stale review",
    "some-author",
    "into main",
    "Merged",
    "CI passed",
  ]);
  expect(merged[0]?.hover).toBe(
    "Changed since your Review: 2 files\nLib/ast.py\nPython/ast.c (whitespace only)",
  );
});

it("says why a closed PR's check can't run, on its row", () => {
  const unchecked: WorkbenchRow = {
    ...stale,
    reasons: [],
    state: "CLOSED",
    your_review: { kind: "cant_check", reason: { kind: "cloning" } },
  };
  expect(detailWords(unchecked)[0]).toEqual({
    words: "Stale review",
    tone: "bad",
    hover: "Tsugiki can't check your Review until the mirror is cloned",
  });
});

it.each<[CantCheck, string]>([
  [{ kind: "cloning" }, "Tsugiki can't check your Review until the mirror is cloned"],
  [
    { kind: "broken" },
    "Tsugiki can't check your Review until the mirror, which git said was broken, is cloned again",
  ],
  [{ kind: "no_git" }, "Tsugiki can't check your Review without git"],
  [
    { kind: "old_git", version: "git version 2.38.1", needed: "2.39" },
    "Tsugiki can't check your Review with git version 2.38.1: it needs git 2.39 or newer",
  ],
  [{ kind: "refused" }, "Tsugiki can't check your Review: GitHub refused to send the commit it covered"],
])("says a Stale review whose check can't run, and its hover says why: %j", (reason, hover) => {
  const unchecked: WorkbenchRow = { ...stale, your_review: { kind: "cant_check", reason } };
  expect(prStateWords(unchecked).at(-1)).toEqual({ words: "Stale review", tone: "bad", hover });
  expect(details(unchecked)).toBe("Stale review · some-author · into main · Open · CI passed");
});

it.each([
  ["FAILURE", "bad"],
  ["ERROR", "bad"],
  ["PENDING", "plain"],
  ["EXPECTED", "plain"],
] as const)("gives CI that is %s the %s tone", (ci, tone) => {
  expect(prStateWords({ ...row, reasons: [], ci })[1]?.tone).toBe(tone);
});

it("names a Backport by its branch and state", () => {
  expect(backportWords({ ...row, state: "MERGED", target_branch: "3.14" })).toBe("3.14 Merged");
});
