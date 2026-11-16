import { expect, type Page, test } from "@playwright/test";
import type {
  Filter_Serialize as Filter,
  InboxCursor_Serialize as InboxCursor,
  ListedItem_Serialize as ListedItem,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const cpython = { screen: "repository", repository, others: [] as string[] } as const;
const workbench = {
  status: "ok" as const,
  data: { items: [] as ListedItem[], count: 0, review_requests: 0, stale_reviews: 0, filters: [] },
};

/** Item `number` of the Inbox, an Issue with one open PR under it, updated `number` minutes past 10. */
const issue = (number: number): ListedItem => {
  const updated_at = new Date(Date.UTC(2026, 10, 4, 10, number)).toISOString();
  const pr = 100_000 + number;
  return {
    attention: null,
    kind: "issue",
    number,
    title: `Issue ${String(number)}`,
    state: "OPEN",
    people: ["issue-author"],
    updated_at,
    pull_requests: [
      {
        kind: "pr",
        number: pr,
        title: `PR for ${String(number)}`,
        author: "some-author",
        state: "OPEN",
        is_draft: false,
        target_branch: "main",
        ci: "SUCCESS",
        updated_at,
        reasons: [],
        your_review: { kind: "unreviewed" },
        seen: { mark: { numbers: [pr], up_to: null }, state: "seen" },
        backports: [],
      },
    ],
    seen: { mark: { numbers: [number, pr], up_to: null }, state: "seen" },
  };
};

/** Items `from` down to `to`, the latest update first. */
const issues = (from: number, to: number) =>
  Array.from({ length: from - to + 1 }, (_, index) => issue(from - index));

/** A page of the Inbox holding `items`, whose next page starts after the last of them while `more` says
 * the cache holds more. */
const inboxPage = (items: ListedItem[], more: boolean) => {
  const last = items.at(-1);
  const older: InboxCursor | null =
    more && last ? { updated_at: last.updated_at, number: last.number } : null;
  return { status: "ok", data: { items, older } } as const;
};

/** Runs palette command `title`, as the user picks it from the palette's commands. */
async function palette(page: Page, title: string) {
  await page.keyboard.press("ControlOrMeta+Shift+P");
  // The box opens with `>` typed, which marks the commands, and `fill` replaces it.
  await page.getByRole("combobox", { name: "Command" }).fill(`>${title}`);
  await expect(page.getByRole("option")).toHaveAccessibleName(title);
  await page.keyboard.press("Enter");
}

/** Opens the Inbox from the Sidebar. */
const openInbox = (page: Page) =>
  page.getByRole("navigation", { name: "Sidebar" }).getByRole("button", { name: "Inbox" }).click();

/** Scrolls the Issue list to its end. */
const scrollToEnd = (page: Page) =>
  page.locator(".list").evaluate((list) => {
    list.scrollTop = list.scrollHeight;
  });

test("the Sidebar and the palette open the Inbox, each Issue with its PRs, in its own order", async ({
  page,
}) => {
  await mockCore(page, { home: cpython, workbench, inbox: inboxPage(issues(3, 1), false) });
  await page.goto("/");

  await openInbox(page);
  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#3 Issue 3" })).toBeVisible();
  await expect(page.getByRole("button", { name: /⇅$/ })).toHaveCount(0);
  await page.getByRole("button", { name: "PRs of #3" }).click();
  await expect(page.getByRole("button", { name: "#100003 PR for 3" })).toBeVisible();

  await page
    .getByRole("navigation", { name: "Sidebar" })
    .getByRole("button", { name: /^Workbench/ })
    .click();
  await expect(page.getByRole("heading", { name: "Workbench" })).toBeVisible();
  await palette(page, "View: Inbox");
  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#1 Issue 1" })).toBeVisible();
});

test("scrolling to the end of the Inbox reads the next page and draws it", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, { home: cpython, workbench, inbox: inboxPage(issues(400, 341), true) });
  await page.goto("/");
  await openInbox(page);
  await expect(page.getByRole("button", { name: "#400 Issue 400" })).toBeVisible();
  await changeAnswer(page, "inbox", inboxPage(issues(340, 281), false));

  // The first page fills the list, so nothing older is read until the user scrolls.
  await expect(page.getByRole("button", { name: "#340 Issue 340" })).toHaveCount(0);
  await scrollToEnd(page);
  await expect(page.getByRole("button", { name: "#340 Issue 340" })).toBeAttached();
  await scrollToEnd(page);

  await expect(page.getByRole("button", { name: "#281 Issue 281" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#341 Issue 341" })).toBeAttached();
  await expect(page.locator(".list li.item")).toHaveCount(120);
});

test("with a cache the size of CPython's, the Inbox draws only the items on screen", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, { home: cpython, workbench, inbox: inboxPage(issues(3000, 1), false) });
  await page.goto("/");
  await openInbox(page);
  // A role query weighs every row's name, which takes seconds over 3000 of them.
  await expect(page.locator('[data-item="#3000"]')).toBeVisible();
  const items = page.locator(".list li.item");
  // An item the browser skips doesn't lay out or draw what's in it.
  const drawn = () =>
    items.evaluateAll((all) =>
      all
        .filter((item) => item.querySelector(".row")?.checkVisibility({ contentVisibilityAuto: true }))
        .map((item) => item.querySelector("code")?.textContent),
    );

  await expect(items).toHaveCount(3000);
  await expect.poll(drawn).toContain("#3000");
  expect((await drawn()).length).toBeLessThan(50);

  // Halfway down the list, among items the browser has never drawn. It works out which items show as it
  // draws the next frame.
  await page.locator(".list").evaluate((list) => {
    list.scrollTop = list.scrollHeight / 2;
  });
  const middle = (numbers: (string | null | undefined)[]) =>
    numbers.some((number) => Math.abs(Number(number?.slice(1)) - 1500) < 50);
  await expect.poll(async () => middle(await drawn())).toBe(true);
  const halfway = await drawn();
  expect(halfway).not.toContain("#3000");
  expect(halfway.length).toBeLessThan(50);
});

test("an item that closed leaves the Inbox at the next refresh", async ({ page }) => {
  await mockCore(page, { home: cpython, workbench, inbox: inboxPage(issues(3, 1), false) });
  await page.goto("/");
  await openInbox(page);
  await expect(page.getByRole("button", { name: "#2 Issue 2" })).toBeVisible();

  await changeAnswer(page, "inbox", inboxPage([issue(3), issue(1)], false));
  await emitFromCore(page, "changed", { repository: repository.name, items: [2] });

  await expect(page.getByRole("button", { name: "#2 Issue 2" })).toHaveCount(0);
  await expect(page.locator(".list li.item")).toHaveCount(2);
});
