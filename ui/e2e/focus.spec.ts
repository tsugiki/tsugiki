import { expect, type Page, test } from "@playwright/test";
import { done, ok, workbenchPage } from "../src/lib/testing/fixtures.ts";
import { home, issue, issueView, prView } from "./fixtures.ts";
import { mockCore } from "./mock-core.ts";
import { themes } from "./themes.ts";

// The pane that holds the focus shows a line along its top, and the list's row under the cursor shows the
// active or the inactive selection (#602). The panes read the focus with :focus-within, which jsdom can't
// draw, so only a browser shows them.

// Light Modern's focus border and selection colours, as e2e/themes.ts gives them.
const FOCUS_BORDER = "rgb(0, 95, 184)";
const ACTIVE = "rgb(232, 232, 232)";
const INACTIVE = "rgb(228, 230, 241)";

/** The panes that show the focus line, by their `data-pane`. */
const lined = (page: Page) =>
  page.evaluate((border) => {
    const panes = [...document.querySelectorAll<HTMLElement>("[data-pane]")];
    return panes
      .filter((pane) => getComputedStyle(pane, "::before").backgroundColor === border)
      .map((pane) => pane.dataset["pane"]);
  }, FOCUS_BORDER);

/** The background of the list's row under the cursor. */
const selection = (page: Page) =>
  page.evaluate(() => {
    const current = document.querySelector('[data-pane="list"] [aria-current]')?.closest(".row");
    return current ? getComputedStyle(current).backgroundColor : undefined;
  });

async function openApp(page: Page): Promise<void> {
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, {
    themes: ok(themes),
    home,
    workbench: workbenchPage([issue], { count: 1 }),
    openIssue: done,
    fetchIssue: done,
    openOnGithub: done,
    issue: ok(issueView),
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: ok(prView()),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();
}

test("the focus line and the selection follow the focus between the panes", async ({ page, browserName }) => {
  await openApp(page);
  // At launch the focus is on no control, and the list takes the keys.
  await expect.poll(() => lined(page)).toEqual(["list"]);

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "Issue view" })).toBeVisible();
  await expect.poll(() => lined(page)).toEqual(["list"]);
  await expect.poll(() => selection(page)).toBe(ACTIVE);

  await page.keyboard.press("ControlOrMeta+Alt+B");
  await expect.poll(() => lined(page)).toEqual(["detail"]);

  await page.keyboard.press("ControlOrMeta+Alt+B");
  await expect.poll(() => lined(page)).toEqual(["list"]);

  // A PR opened from its row, then Esc with the list hidden: the Issue's view takes the focus.
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+Alt+B");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("region", { name: "Issue view" })).toBeVisible();
  await expect.poll(() => lined(page)).toEqual(["detail"]);

  // Back in the list, Alt+← goes back to the PR and puts the focus on its row.
  await page.keyboard.press("ControlOrMeta+Alt+B");
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await expect.poll(() => lined(page)).toEqual(["list"]);

  // WebKit doesn't focus a button it clicks, as Safari doesn't, so a click moves the focus only in Chromium.
  if (browserName === "chromium") {
    await page.getByRole("region", { name: "PR view" }).getByRole("tab", { name: "Files" }).click();
    await expect.poll(() => lined(page)).toEqual(["detail"]);
    await expect.poll(() => selection(page)).toBe(INACTIVE);
    // ↑ ↓ go where the line is: they scroll the Detail pane, and the focus stays there.
    await page.keyboard.press("ArrowDown");
    await expect.poll(() => lined(page)).toEqual(["detail"]);
  }
});

test("the palette on top, or another app in front, leaves every pane inactive", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ArrowDown");
  await expect.poll(() => lined(page)).toEqual(["list"]);

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(page.getByRole("combobox", { name: "Command" })).toBeFocused();
  await expect.poll(() => lined(page)).toEqual([]);
  await expect.poll(() => selection(page)).toBe(INACTIVE);

  await page.keyboard.press("Escape");
  await expect.poll(() => lined(page)).toEqual(["list"]);

  await page.evaluate(() => window.dispatchEvent(new Event("blur")));
  await expect.poll(() => lined(page)).toEqual([]);
  await expect.poll(() => selection(page)).toBe(INACTIVE);

  await page.evaluate(() => window.dispatchEvent(new Event("focus")));
  await expect.poll(() => lined(page)).toEqual(["list"]);
});
