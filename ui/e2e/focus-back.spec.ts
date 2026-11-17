import { expect, type Page, test } from "@playwright/test";
import type { ListedItem_Serialize as ListedItem } from "../src/lib/data/bindings.ts";
import { workbenchPage, workbenchRow } from "../src/lib/testing/fixtures.ts";
import { home } from "./fixtures.ts";
import { mockCore } from "./mock-core.ts";

// A dialog gives the focus back as it closes, and the browser then acts on the press that closed it. jsdom
// has nothing that acts on a press, so only a browser shows the focus staying where it went back to.

const own: ListedItem = {
  attention: null,
  kind: "standalone_pr",
  ...workbenchRow({
    number: 157178,
    title: "GH-156425: Clear GC frame on early return",
    author: "octocat",
    is_draft: true,
    target_branch: "3.14",
    ci: "FAILURE",
    updated_at: "2026-11-04T09:00:00Z",
    reasons: ["own_pr"],
  }),
  backports: [],
};

/** Opens the app on a Repository with one row. */
async function openApp(page: Page): Promise<void> {
  await mockCore(page, {
    status: { state: "online", login: "octocat" },
    home,
    workbench: workbenchPage([own], { count: 1 }),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: /#157178/ })).toBeVisible();
}

test("a press outside the command palette gives the focus back to the row", async ({ page }) => {
  await openApp(page);
  const row = page.getByRole("button", { name: /#157178/ });
  await page.keyboard.press("ArrowDown");
  await expect(row).toBeFocused();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await expect(page.getByRole("combobox", { name: "Command" })).toBeFocused();
  await page.getByTestId("palette-scrim").click({ position: { x: 5, y: 5 } });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(row).toBeFocused();
});

test("a press outside Add repository gives the focus back to what had it before the picker", async ({
  page,
}) => {
  await openApp(page);
  const row = page.getByRole("button", { name: /#157178/ });
  await page.keyboard.press("ArrowDown");
  await expect(row).toBeFocused();

  await page.keyboard.press("ControlOrMeta+R");
  await page.getByRole("combobox", { name: "Repository" }).fill("add");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("textbox", { name: "Repository" })).toBeFocused();

  await page.getByTestId("add-repository-scrim").click({ position: { x: 5, y: 5 } });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(row).toBeFocused();
});
