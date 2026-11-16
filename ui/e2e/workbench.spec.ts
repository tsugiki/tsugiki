import { expect, type Page, test } from "@playwright/test";
import type {
  Filter,
  ListedItem_Serialize as ListedItem,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";

const mine = { name: "Mine", query: "is:open author:@me", count: 1, problem: null };
const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const cpython = { screen: "repository", repository, others: [] as string[] } as const;

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
  your_review: { kind: "unreviewed" },
  seen: { mark: { numbers: [160662], up_to: null }, state: "seen" },
};

const review: ListedItem = {
  attention: { since: "2026-11-04T10:00:00Z", why: [{ kind: "review_requested" }] },
  kind: "issue",
  number: 112564,
  title: "GIL drop during collection",
  state: "OPEN",
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
          your_review: { kind: "unreviewed" },
          seen: { mark: { numbers: [160700], up_to: null }, state: "seen" },
          backport: "bot",
        },
      ],
    },
  ],
  seen: {
    mark: { numbers: [112564, 160662, 160700], up_to: null },
    state: "seen",
  },
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
  your_review: { kind: "unreviewed" },
  seen: { mark: { numbers: [157178], up_to: null }, state: "seen" },
  backports: [],
};

test("the Workbench lists what waits on the user", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [review, own], count: 2, review_requests: 0, stale_reviews: 0, filters: [] },
    },
  });
  await page.goto("/");

  const rows = page.getByRole("listitem");
  await expect(rows).toHaveCount(2);
  await page.getByRole("button", { name: "PRs of #112564" }).click();
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0).locator("strong")).toHaveText("#112564 GIL drop during collection");
  await expect(rows.nth(0)).toContainText("Review requested from you");
  await expect(rows.nth(0).locator(".people")).toHaveText("issue-author, some-author");
  await expect(rows.nth(0).locator(".count").first()).toHaveText("2 PRs");
  await expect(rows.nth(1)).toContainText("#160662 gh-112564: Fix UAF for gc debug print");
  await expect(rows.nth(1)).toContainText("Review request · some-author · into main · Open · CI passed");
  await expect(rows.nth(2)).toContainText("Bot backport · some-author · into 3.14 · Merged · CI passed");
  await expect(rows.nth(3)).toContainText("Own PR · octocat · into 3.14 · Open · Draft · CI failed");
});

test("an item that Needs attention shows an accent bar and title, and a long title ends in an ellipsis", async ({
  page,
}) => {
  const long = { ...own, title: "A title long enough to run past the list's edge ".repeat(4) };
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [review, long], count: 2, review_requests: 0, stale_reviews: 0, filters: [] },
    },
  });
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/");

  const [unread, read] = [page.getByRole("listitem").nth(0), page.getByRole("listitem").nth(1)];
  const title = (item: typeof unread) => item.locator("button.title");
  const accent = await page.evaluate(() => {
    const probe = document.body.appendChild(document.createElement("i"));
    probe.style.color = "var(--accent)";
    return getComputedStyle(probe).color;
  });
  await expect(unread).toHaveCSS("border-left-color", accent);
  await expect(title(unread)).toHaveCSS("color", accent);
  await expect(unread.locator(".people")).toHaveCSS("font-weight", "600");
  await expect(title(read)).not.toHaveCSS("color", accent);
  await expect(read).toHaveCSS("border-left-color", "rgba(0, 0, 0, 0)");
  expect(await title(read).evaluate((button) => button.scrollWidth > button.clientWidth)).toBe(true);
  await expect(title(read)).toHaveCSS("text-overflow", "ellipsis");
});

test("an item with nothing waiting sits in the one list below what does, and Alt+← goes back to it", async ({
  page,
}) => {
  const done = { status: "ok", data: null } as const;
  const ownView = { ...reviewRow, ...own, content: null, shares_review_of: null };
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [review, own], count: 1, review_requests: 0, stale_reviews: 0, filters: [] },
    },
    openIssue: done,
    fetchIssue: done,
    issue: {
      status: "ok",
      data: {
        ...review,
        merged: [],
        merged_count: 0,
        author: "issue-author",
        assignees: [],
        labels: [],
        content: null,
        needs_attention: [],
      },
    },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: { status: "ok", data: ownView },
  });
  await page.goto("/");
  const ownRow = page
    .getByRole("list")
    .getByRole("button", { name: "#157178 GH-156425: Clear GC frame on early return" });
  await expect(ownRow).toBeVisible();
  await expect(page.getByRole("button", { name: /quiet/i })).toHaveCount(0);

  await ownRow.click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await page.getByRole("button", { name: "#112564 GIL drop during collection" }).click();
  await expect(page.getByRole("region", { name: "Issue view" })).toBeVisible();

  await page.keyboard.press("Alt+ArrowLeft");

  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await expect(ownRow).toBeFocused();
});

test("the first sync's rows appear as the core saves them", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
    },
  });
  await page.goto("/");
  await expect(page.getByText(/Nothing here yet/)).toBeVisible();

  await changeAnswer(page, "workbench", {
    status: "ok",
    data: { items: [own], count: 1, review_requests: 0, stale_reviews: 0, filters: [] },
  });
  await emitFromCore(page, "changed", { repository: "tsugiki/tsugiki", items: [1] });
  await emitFromCore(page, "changed", { repository: repository.name, items: [1] });

  await expect(page.getByRole("listitem")).toHaveCount(1);
  await expect(page.getByText(/Nothing here yet/)).toBeHidden();
});

test("the Sidebar lists the Filters, and selecting one shows its Issues", async ({ page }) => {
  const whole = {
    items: [review, own],
    count: 2,
    review_requests: 0,
    stale_reviews: 0,
    filters: [mine],
  };
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: whole } });
  await page.goto("/");

  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await expect(sidebar.getByRole("button", { name: "Workbench 2" })).toHaveAttribute("aria-current", "true");
  await expect(page.getByRole("listitem")).toHaveCount(2);

  await changeAnswer(page, "workbench", { status: "ok", data: { ...whole, items: [own] } });
  await sidebar.getByRole("button", { name: "Mine 1" }).click();

  await expect(page.getByRole("listitem")).toHaveCount(1);
  await expect(page.getByRole("listitem")).toContainText("#157178 GH-156425: Clear GC frame on early return");
  await expect(sidebar.getByRole("button", { name: "Mine 1" })).toHaveAttribute("aria-current", "true");
});

test("a drag past the Sidebar's least width folds it to icons, and a click on one brings it back", async ({
  page,
}) => {
  const whole = {
    items: [review, own],
    count: 2,
    review_requests: 0,
    stale_reviews: 0,
    filters: [mine],
  };
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: whole } });
  await page.goto("/");
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await expect(sidebar.getByRole("button", { name: "Workbench 2" })).toBeVisible();
  const width = async () => (await sidebar.boundingBox())?.width ?? 0;
  const edge = (await page.getByRole("separator", { name: "Sidebar width" }).boundingBox()) ?? { x: 0, y: 0 };

  // From 200 px, 70 px left is past the least width and short of where it folds, and 90 px is past that.
  await page.mouse.move(edge.x + 8, edge.y + 20);
  await page.mouse.down();
  await page.mouse.move(edge.x + 8 - 70, edge.y + 20, { steps: 4 });
  const short = await width();
  await page.mouse.move(edge.x + 8 - 90, edge.y + 20, { steps: 4 });
  await page.mouse.up();
  await expect(sidebar).toHaveClass(/icons/);
  const strip = await width();
  await expect(sidebar.getByRole("button", { name: "Workbench (2)" })).toHaveText("2");
  await page.keyboard.press("ControlOrMeta+B");
  await expect(sidebar).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+B");
  await expect(sidebar).toHaveClass(/icons/);
  await sidebar.getByRole("button", { name: "Mine (1)" }).click();

  await expect(sidebar.getByRole("button", { name: "Mine 1" })).toHaveAttribute("aria-current", "true");
  expect([short, strip, await width()]).toEqual([140, 48, 200]);
});

test("a change keeps the selected Filter and the place the user had scrolled to", async ({ page }) => {
  const many = Array.from({ length: 80 }, (_, at) => ({ ...own, number: 1000 + at }));
  const whole = { items: many, count: 80, review_requests: 0, stale_reviews: 0, filters: [mine] };
  await page.setViewportSize({ width: 800, height: 400 });
  await mockCore(page, { home: cpython, workbench: { status: "ok", data: whole } });
  await page.goto("/");

  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await sidebar.getByRole("button", { name: "Mine 1" }).click();
  await expect(page.getByRole("listitem").first()).toBeVisible();
  const list = page.locator(".list");
  const scrolled = () => list.evaluate((element) => element.scrollTop);
  await list.evaluate((element) => {
    element.scrollTop = 600;
  });
  await expect.poll(scrolled).toBe(600);

  const fresher = many.map((item, at) => (at === 5 ? { ...item, title: "A title that changed" } : item));
  await changeAnswer(page, "workbench", { status: "ok", data: { ...whole, items: fresher } });
  await emitFromCore(page, "changed", { repository: repository.name, items: [1005] });

  await expect(page.getByText("A title that changed")).toBeAttached();
  expect(await scrolled()).toBe(600);
  await expect(sidebar.getByRole("button", { name: "Mine 1" })).toHaveAttribute("aria-current", "true");
});

/** Thirty items with nothing waiting, `#1000` to `#1029`, in a window that shows only a few of them. */
const thirty = Array.from({ length: 30 }, (_, at): ListedItem => {
  const number = 1000 + at;
  return {
    ...own,
    number,
    title: `Item ${String(number)}`,
    seen: { ...own.seen, mark: { numbers: [number], up_to: null } },
  };
});

/** Opens `#1020` from the list in the Detail pane, with its row on the screen part way down the list. */
async function openFarDown(page: Page) {
  const done = { status: "ok", data: null } as const;
  const whole = { items: thirty, count: 30, review_requests: 0, stale_reviews: 0, filters: [] };
  await page.setViewportSize({ width: 900, height: 400 });
  await mockCore(page, {
    home: cpython,
    workbench: { status: "ok", data: whole },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: {
      status: "ok",
      data: { ...reviewRow, number: 1020, title: "Item 1020", content: null, shares_review_of: null },
    },
  });
  await page.goto("/");
  const list = page.locator(".list");
  const row = list.getByRole("button", { name: "#1020 Item 1020" });
  await row.scrollIntoViewIfNeeded();
  await row.click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  /** A change that moves item `number` to the top of the list, as an update does By last update. */
  const moveToTop = async (number = 1020) => {
    const at = number - 1000;
    const moved = [thirty[at] as ListedItem, ...thirty.filter((_, other) => other !== at)];
    await changeAnswer(page, "workbench", { status: "ok", data: { ...whole, items: moved } });
    await emitFromCore(page, "changed", { repository: repository.name, items: [number] });
    await expect(list.getByRole("listitem").first()).toContainText(
      `#${String(number)} Item ${String(number)}`,
    );
  };
  /** The place of each item's row on the screen, by its name. The browser's scroll anchoring keeps a row
   * where it was as rows above it move, so the place on the screen, not `scrollTop`, says whether the list
   * moved under the user. */
  const onScreen = () =>
    list.evaluate((element) => {
      const shown = element.getBoundingClientRect();
      const rows = Array.from(element.querySelectorAll(".item > .row"), (row) => {
        const { top, bottom } = row.getBoundingClientRect();
        return { name: row.getAttribute("data-row"), top, bottom };
      });
      return rows.filter((row) => row.bottom > shown.top && row.top < shown.bottom);
    });
  /** The top of row `name` on the screen. */
  const topOf = async (name: string | null) => (await onScreen()).find((row) => row.name === name)?.top;
  /** Expects row `name` on the screen within half a pixel of `top`: a HiDPI screen can land an anchored
   * scroll a fraction of a pixel off, and a scroll the browser didn't anchor moves the row a whole row. */
  const expectAt = async (name: string | null, top: number | undefined) => {
    expect(top).toBeDefined();
    expect(await topOf(name)).toBeCloseTo(top ?? Number.NaN, 0);
  };
  return { list, row, onScreen, topOf, expectAt, moveToTop };
}

test("a change that moves the Detail pane's row off the screen scrolls the list to it, and the row keeps the focus", async ({
  page,
}) => {
  const { row, moveToTop } = await openFarDown(page);
  // WebKit doesn't focus a button it clicks, as Safari doesn't.
  await row.focus();

  await moveToTop();

  await expect(row).toBeInViewport();
  await expect(row).toHaveAttribute("aria-current", "true");
  await expect(row).toBeFocused();
});

test("a change that moves the Detail pane's row off the screen scrolls the list, and the Detail pane keeps the focus", async ({
  page,
}) => {
  const { row, moveToTop } = await openFarDown(page);
  const tab = page.getByRole("region", { name: "PR view" }).getByRole("tab", { name: "Conversation" });
  await tab.focus();

  await moveToTop();

  await expect(row).toBeInViewport();
  await expect(tab).toBeFocused();
});

test("a change that moves the Detail pane's item leaves the scroll alone while the cursor is on another row", async ({
  page,
}) => {
  const { row, topOf, expectAt, moveToTop } = await openFarDown(page);
  await row.focus();
  await page.keyboard.press("ArrowDown");
  const cursor = page.getByRole("button", { name: "#1021 Item 1021" });
  await expect(cursor).toBeFocused();
  // The row above the Detail pane's stays put. The rows below it close the gap it leaves, as they must.
  const above = await topOf("#1019");
  expect(above).toBeDefined();

  await moveToTop();

  await expectAt("#1019", above);
  await expect(row).not.toBeInViewport();
  await expect(cursor).toBeFocused();
});

test("a change that moves the Detail pane's row leaves the scroll alone once the user scrolled the row away", async ({
  page,
}) => {
  const { list, row, onScreen, expectAt, moveToTop } = await openFarDown(page);
  await list.evaluate((element) => {
    element.scrollTop = element.scrollHeight;
  });
  await expect(row).not.toBeInViewport();
  const before = await onScreen();

  await moveToTop();

  expect(before.length).toBeGreaterThan(0);
  for (const { name, top } of before) await expectAt(name, top);
  await expect(row).not.toBeInViewport();
});

test("a change that moves the focused row under the cursor gives it back the focus, though the Detail pane shows another item", async ({
  page,
}) => {
  const { row, topOf, expectAt, moveToTop } = await openFarDown(page);
  await row.focus();
  await page.keyboard.press("ArrowDown");
  const cursor = page.getByRole("button", { name: "#1021 Item 1021" });
  await expect(cursor).toBeFocused();
  const above = await topOf("#1020");
  expect(above).toBeDefined();

  await moveToTop(1021);

  await expect(cursor).toBeFocused();
  await expect(cursor).toHaveAttribute("aria-current", "true");
  await expectAt("#1020", above);
});

test("the status bar says when the Repository last synced, and a click asks again", async ({ page }) => {
  await mockCore(page, {
    home: cpython,
    workbench: {
      status: "ok",
      data: { items: [], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
    },
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
