import { expect, test } from "@playwright/test";
import type {
  Filter,
  ListedItem_Serialize as ListedItem,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";

const mine = { name: "Mine", query: "is:open author:@me", count: 1, problem: null };
const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const cpython = { screen: "repository", repository } as const;

const reviewRow: WorkbenchRow = {
  number: 160662,
  title: "gh-112564: Fix UAF for gc debug print",
  author: "some-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: ["review_request"],
};

const review: ListedItem = {
  attention: { since: "2026-11-04T10:00:00Z", why: [{ kind: "review_requested" }] },
  kind: "issue",
  number: 112564,
  title: "GIL drop during collection",
  people: ["issue-author", "some-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [
    {
      kind: "pr",
      ...reviewRow,
      backports: [
        {
          ...reviewRow,
          number: 160700,
          title: "[3.14] gh-112564: Fix UAF for gc debug print (GH-160662)",
          state: "MERGED",
          target_branch: "3.14",
          reasons: [],
          backport: "bot",
        },
      ],
    },
  ],
};

const own: ListedItem = {
  attention: null,
  kind: "standalone_pr",
  number: 157178,
  title: "GH-156425: Clear GC frame on early return",
  author: "octocat",
  state: "OPEN",
  is_draft: true,
  target_branch: "3.14",
  ci: "FAILURE",
  updated_at: "2026-11-04T09:00:00Z",
  reasons: ["own_pr"],
  backports: [],
};

test("the Workbench lists what waits on the user", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [review, own], quiet: [], count: 2, review_requests: 0, filters: [] },
    },
  });
  await page.goto("/");

  const rows = page.getByRole("listitem");
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0).locator("strong")).toHaveText("#112564 GIL drop during collection");
  await expect(rows.nth(0)).toContainText("Review requested from you");
  await expect(rows.nth(0)).toContainText("issue-author, some-author · 2 PRs · ");
  await expect(rows.nth(1)).toContainText("#160662 gh-112564: Fix UAF for gc debug print");
  await expect(rows.nth(1)).toContainText("Review request · some-author · into main · Open · CI passed");
  await expect(rows.nth(2)).toContainText("Bot backport · some-author · into 3.14 · Merged · CI passed");
  await expect(rows.nth(3)).toContainText("Own PR · octocat · into 3.14 · Open · Draft · CI failed · ");
});

test("the first sync's rows appear as the core saves them", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: { status: "ok", data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] } },
  });
  await page.goto("/");
  await expect(page.getByText(/Nothing here yet/)).toBeVisible();

  await changeAnswer(page, "workbench", {
    status: "ok",
    data: { items: [own], quiet: [], count: 1, review_requests: 0, filters: [] },
  });
  await emitFromCore(page, "changed", { repository: "tsugiki/tsugiki", items: [1] });
  await emitFromCore(page, "changed", { repository: repository.name, items: [1] });

  await expect(page.getByRole("listitem")).toHaveCount(1);
  await expect(page.getByText(/Nothing here yet/)).toBeHidden();
});

test("the Sidebar lists the Filters, and selecting one shows its Issues", async ({ page }) => {
  const whole = { items: [review, own], quiet: [], count: 2, review_requests: 0, filters: [mine] };
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: whole } });
  await page.goto("/");

  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await expect(sidebar.getByRole("button", { name: "Workbench 2" })).toHaveAttribute("aria-current", "true");
  await expect(page.getByRole("listitem")).toHaveCount(4);

  await changeAnswer(page, "workbench", { status: "ok", data: { ...whole, items: [own] } });
  await sidebar.getByRole("button", { name: "Mine 1" }).click();

  await expect(page.getByRole("listitem")).toHaveCount(1);
  await expect(page.getByRole("listitem")).toContainText("#157178 GH-156425: Clear GC frame on early return");
  await expect(sidebar.getByRole("button", { name: "Mine 1" })).toHaveAttribute("aria-current", "true");
});

test("a change keeps the selected Filter and the place the user had scrolled to", async ({ page }) => {
  const many = Array.from({ length: 80 }, (_, at) => ({ ...own, number: 1000 + at }));
  const whole = { items: many, quiet: [], count: 80, review_requests: 0, filters: [mine] };
  await page.setViewportSize({ width: 800, height: 400 });
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: whole } });
  await page.goto("/");

  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await sidebar.getByRole("button", { name: "Mine 1" }).click();
  await expect(page.getByRole("listitem").first()).toBeVisible();
  await page.evaluate(() => {
    window.scrollTo(0, 600);
  });
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBe(600);

  const fresher = many.map((item, at) => (at === 5 ? { ...item, title: "A title that changed" } : item));
  await changeAnswer(page, "workbench", { status: "ok", data: { ...whole, items: fresher } });
  await emitFromCore(page, "changed", { repository: repository.name, items: [1005] });

  await expect(page.getByText("A title that changed")).toBeAttached();
  expect(await page.evaluate(() => window.scrollY)).toBe(600);
  await expect(sidebar.getByRole("button", { name: "Mine 1" })).toHaveAttribute("aria-current", "true");
});

test("the status bar says when the Repository last synced, and a click asks again", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: { status: "ok", data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] } },
    status: { state: "online", login: "octocat" },
    lastSync: { repository: repository.name, at: null },
  });
  await page.goto("/");

  await page.getByRole("button", { name: "Not synced yet" }).click();
  await emitFromCore(page, "last-sync-changed", {
    repository: repository.name,
    at: new Date().toISOString(),
  });

  await expect(page.getByRole("button", { name: "Synced just now" })).toBeVisible();
});

test("the Workbench says why it can't read", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: { status: "error", error: { kind: "internal", message: "The database stopped." } },
  });
  await page.goto("/");
  await expect(page.getByRole("alert")).toHaveText(
    "Tsugiki can't read this Workbench: The database stopped.",
  );
});
