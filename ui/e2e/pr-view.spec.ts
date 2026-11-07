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
  await page.keyboard.press("ControlOrMeta+PageUp");
  await expect(pane).toContainText("The fix");

  await page.keyboard.press("Escape");

  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("Issue 7");
  await expect(pane).toHaveCount(0);
});
