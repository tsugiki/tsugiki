import { expect, type Page, test } from "@playwright/test";
import type {
  InboxCursor_Serialize as InboxCursor,
  ListedItem_Serialize as ListedItem,
} from "../src/lib/data/bindings.ts";
import { inboxPage, workbenchPage, workbenchRow } from "../src/lib/testing/fixtures.ts";
import { home, palette, repository } from "./fixtures.ts";
import { changeAnswer, callsOf, emitFromCore, mockCore } from "./mock-core.ts";

const workbench = workbenchPage([] as ListedItem[]);

/** A listed item the cache holds, as against a placeholder a search found (#465). */
type CachedItem = Exclude<ListedItem, { kind: "placeholder" }>;

/** Item `number` of the Inbox, an Issue with one open PR under it, updated `number` minutes past 10. */
const issue = (number: number): CachedItem => {
  const updated_at = new Date(Date.UTC(2026, 10, 4, 10, number)).toISOString();
  const pr = 100_000 + number;
  return {
    attention: null,
    key: { kind: "issue", number },
    hidden: null,
    unpin: "stays",
    kind: "issue",
    number,
    title: `Issue ${String(number)}`,
    state: "OPEN",
    people: ["issue-author"],
    mute_author: { kind: "mute", login: "issue-author" },
    pinned: false,
    updated_at,
    issue_updated_at: updated_at,
    pull_requests: [
      {
        kind: "pr",
        ...workbenchRow(pr, { title: `PR for ${String(number)}`, author: "some-author", updated_at }),
        backports: [],
      },
    ],
    seen: { mark: { numbers: [number, pr], up_to: null, findings: [] }, state: "seen" },
  };
};

/** Items `from` down to `to`, the latest update first. */
const issues = (from: number, to: number) =>
  Array.from({ length: from - to + 1 }, (_, index) => issue(from - index));

/** A page of the Inbox holding `items`, whose next page starts after the last of them while `more` says
 * the cache holds more. */
const pageOf = (items: ListedItem[], more: boolean) => {
  const last = items.at(-1);
  const older: InboxCursor | null =
    more && last ? { updated_at: last.updated_at, number: last.number } : null;
  return inboxPage(items, older);
};

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
  await mockCore(page, { home, workbench, inbox: pageOf(issues(3, 1), false) });
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

test("typing in the search box narrows the Inbox on the cache, and an empty box brings the whole list back", async ({
  page,
}) => {
  await mockCore(page, { home, workbench, inbox: pageOf(issues(3, 1), false) });
  await page.goto("/");
  await openInbox(page);
  await expect(page.getByRole("button", { name: "#3 Issue 3" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#1 Issue 1" })).toBeVisible();
  const search = page.getByRole("searchbox", { name: "Search the Inbox" });

  // Each key asks the core, which answers on the cache with the rows it found.
  await changeAnswer(page, "inbox", pageOf([issue(3)], false));
  await search.press("I");
  await expect.poll(() => callsOf(page, "inbox")).toBe(2);
  await search.fill("Issue 3");
  await expect(page.getByRole("button", { name: "#3 Issue 3" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#1 Issue 1" })).toHaveCount(0);

  // A form the cache doesn't read lists nothing and says the search needs GitHub.
  await changeAnswer(page, "inbox", inboxPage([], null, "off", true));
  await search.fill("review:approved");
  await expect(page.locator(".list").getByRole("status")).toContainText("needs GitHub");
  await expect(page.locator(".list li.item")).toHaveCount(0);

  // Clearing the box brings the whole list back.
  await changeAnswer(page, "inbox", pageOf(issues(3, 1), false));
  await search.fill("");
  await expect(page.getByRole("button", { name: "#3 Issue 3" })).toBeVisible();
  await expect(page.getByRole("button", { name: "#1 Issue 1" })).toBeVisible();
});

test("Enter sends the whole search to GitHub, and its rows show with a placeholder and the 1,000 line", async ({
  page,
}) => {
  await mockCore(page, { home, workbench, inbox: pageOf(issues(2, 1), false) });
  await page.goto("/");
  await openInbox(page);
  const search = page.getByRole("searchbox", { name: "Search the Inbox" });

  const placeholder: ListedItem = {
    attention: null,
    key: { kind: "pull_request", number: 4242 },
    hidden: null,
    unpin: "stays",
    kind: "placeholder",
    number: 4242,
    item_kind: "pull_request",
    updated_at: "2026-11-04T10:00:00Z",
  };
  const answered = inboxPage([placeholder, issue(1)], null, "off", false, {
    shown: 1000,
    total: 1712,
  });
  await changeAnswer(page, "inbox", answered);
  await search.fill("review:approved");
  // Enter sends the box's whole text to GitHub at once, without waiting for the pause in typing, and drops
  // the pause's timer so only this one search goes.
  await search.press("Enter");
  await expect.poll(() => callsOf(page, "searchInbox")).toBe(1);

  // A result the cache lacks holds its place, and a search past 1,000 says how many it showed.
  await expect(page.getByText("PR not in the cache yet")).toBeVisible();
  await expect(page.getByText("#4242")).toBeVisible();
  await expect(page.locator(".list").getByRole("status")).toHaveText(
    "1,000 of 1,712 shown. Narrow the search, or save it as a Filter.",
  );
});

test("scrolling to the end of the Inbox reads the next page and draws it", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, { home, workbench, inbox: pageOf(issues(400, 341), true) });
  await page.goto("/");
  await openInbox(page);
  await expect(page.getByRole("button", { name: "#400 Issue 400" })).toBeVisible();
  await changeAnswer(page, "inbox", pageOf(issues(340, 281), false));

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
  await mockCore(page, { home, workbench, inbox: pageOf(issues(3000, 1), false) });
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

test("the Detail pane's item comes on screen from far down the Inbox, past items never drawn", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  // Fewer than the 3000 above, as each click weighs every row, and still some 130 screens of them.
  const count = 1000;
  await mockCore(page, { home, workbench, inbox: pageOf(issues(count, 1), false) });
  await page.goto("/");
  await openInbox(page);
  const row = (number: number) => page.locator(`[data-item="#${String(number)}"]`);
  await row(count).click();
  await expect(page.getByRole("region", { name: "Issue view" })).toBeVisible();

  // A change takes the item from the top of the list to its end.
  await changeAnswer(page, "inbox", pageOf([...issues(count - 1, 1), issue(count)], false));
  await emitFromCore(page, "changed", { repository: repository.name, items: [count] });
  await expect(
    page
      .locator(".list li.item")
      .last()
      .locator(`[data-item="#${String(count)}"]`),
  ).toHaveCount(1);
  await expect(row(count)).toBeInViewport();

  // History goes back to it from the top of the list, and on again.
  await page.locator(".list").evaluate((list) => {
    list.scrollTop = 0;
  });
  await row(count - 1).click();
  await expect(row(count)).not.toBeInViewport();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(row(count)).toBeInViewport();
  await expect(row(count)).toBeFocused();
  await page.keyboard.press("Alt+ArrowRight");
  await expect(row(count - 1)).toBeInViewport();
});

test("Show muted greys a Muted author's rows, and turning it off lists the Inbox without them", async ({
  page,
}) => {
  const muted: CachedItem = { ...issue(2), mute_author: { kind: "unmute", login: "issue-author" } };
  await mockCore(page, {
    home,
    workbench,
    inbox: inboxPage([issue(3), muted, issue(1)], null, "on"),
    switchShowMuted: { status: "ok", data: null },
  });
  await page.goto("/");
  await openInbox(page);
  const show = page.getByRole("checkbox", { name: "Show muted" });
  await expect(show).toBeChecked();
  const line = (number: number) => page.locator(`[data-row="#${String(number)}"]`);
  await expect(line(2)).toHaveCSS("opacity", "0.55");
  await expect(line(3)).toHaveCSS("opacity", "1");

  // The core has saved the switch once it answers, and the Inbox read after that leaves the row out.
  await changeAnswer(page, "inbox", inboxPage([issue(3), issue(1)], null, "off"));
  await show.click();

  await expect(page.getByRole("button", { name: "#2 Issue 2" })).toHaveCount(0);
  await expect(show).not.toBeChecked();
});

test("an item that closed leaves the Inbox at the next refresh", async ({ page }) => {
  await mockCore(page, { home, workbench, inbox: pageOf(issues(3, 1), false) });
  await page.goto("/");
  await openInbox(page);
  await expect(page.getByRole("button", { name: "#2 Issue 2" })).toBeVisible();

  await changeAnswer(page, "inbox", pageOf([issue(3), issue(1)], false));
  await emitFromCore(page, "changed", { repository: repository.name, items: [2] });

  await expect(page.getByRole("button", { name: "#2 Issue 2" })).toHaveCount(0);
  await expect(page.locator(".list li.item")).toHaveCount(2);
});
