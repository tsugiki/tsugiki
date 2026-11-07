import { expect, test } from "@playwright/test";
import type {
  Filter,
  ListedItem_Serialize as ListedItem,
  PrView_Serialize as PrView,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;

const row: WorkbenchRow = {
  number: 10,
  title: "gh-7: Fix it",
  author: "pr-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: null,
};

const issue: ListedItem = {
  kind: "issue",
  number: 7,
  title: "Issue 7",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ ...row, kind: "pr", backports: [] }],
  attention: null,
};

const view: PrView = {
  ...row,
  content: {
    body_html: "<p>The fix</p>",
    comments: [],
    earlier_comments: 0,
    reviews: [
      {
        author: "some-reviewer",
        state: "APPROVED",
        commit: "0123abcdef",
        submitted_at: "2026-11-04T10:00:00Z",
        body_html: "",
        hidden: null,
        threads: [],
      },
    ],
    earlier_reviews: 0,
  },
};

const done = { status: "ok", data: null } as const;

test("a PR in an Issue's tree opens in the Detail pane, switches tabs, and Esc goes back", async ({
  page,
}) => {
  await mockCore(page, {
    home: { screen: "repository", repository },
    workbench: {
      status: "ok",
      data: { items: [issue], quiet: [], count: 0, review_requests: 0, filters: [] },
    },
    openIssue: done,
    fetchIssue: done,
    issue: { status: "ok", data: { ...issue, author: "issue-author", content: null } },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: { status: "ok", data: view },
    pullRequestFiles: {
      status: "ok",
      data: {
        state: "ready",
        head: "0123abcdef",
        files: [
          {
            path: "NEWS",
            old_path: null,
            change: { kind: "lines", added: 1, removed: 0 },
            row: 0,
            columns: 8,
          },
        ],
        rows: 3,
      },
    },
    pullRequestRows: {
      status: "ok",
      data: [
        { kind: "file", path: "NEWS", old_path: null, change: { kind: "lines", added: 1, removed: 0 } },
        { kind: "hunk", text: "@@ -0,0 +1 @@" },
        { kind: "added", new: 1, text: "The fix" },
      ],
    },
    pullRequestSpans: {
      status: "ok",
      data: [
        [],
        [],
        [
          { start: 0, end: 3, highlight: "heading" },
          { start: 4, end: 7, highlight: "keyword" },
        ],
      ],
    },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "#10 gh-7: Fix it" })).toBeFocused();
  await page.keyboard.press("Enter");

  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toContainText("The fix");
  await expect(pane).toContainText("some-reviewer approved at 0123abc");
  await page.keyboard.press("ControlOrMeta+PageDown");
  await expect(pane.getByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
  const diff = pane.getByRole("region", { name: "Diff" });
  await expect(diff).toContainText("NEWS+1 −0");
  await expect(diff).toContainText("1+The fix");
  // github.com's colors, light as the window is now, and dark under a dark theme.
  const keyword = diff.locator(".hl-keyword");
  await expect(keyword).toHaveText("fix");
  await expect(keyword).toHaveCSS("color", "rgb(207, 34, 46)");
  await expect(diff.locator(".hl-heading")).toHaveCSS("font-weight", "600");
  await page.evaluate(() => {
    document.documentElement.dataset["theme"] = "dark";
  });
  await expect(keyword).toHaveCSS("color", "rgb(255, 123, 114)");
  await page.keyboard.press("ControlOrMeta+PageUp");
  await expect(pane).toContainText("The fix");

  await page.keyboard.press("Escape");

  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("Issue 7");
  await expect(pane).toHaveCount(0);
});
