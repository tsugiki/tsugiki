import { expect, test } from "@playwright/test";
import type { Filter_Serialize as Filter } from "../src/lib/data/bindings.ts";
import { ok, openOn, workbenchPage } from "../src/lib/testing/fixtures.ts";
import { repository as cpython } from "./fixtures.ts";
import { changeAnswer, mockCore } from "./mock-core.ts";

const peps = { name: "python/peps", conventions: "github", filters: [] as Filter[] } as const;
const workbench = workbenchPage([]);

test("Ctrl+R and Enter switch back to the Repository opened before", async ({ page }) => {
  await mockCore(page, {
    home: openOn(cpython, [peps.name, "python/devguide"]),
    openRepository: ok(openOn(peps, [cpython.name])),
    workbench,
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "python/cpython", exact: true })).toBeVisible();

  await page.keyboard.press("ControlOrMeta+R");
  const picker = page.getByRole("dialog", { name: "Repository picker" });
  await expect(picker.getByRole("option")).toHaveText([
    "Home",
    "python/cpython open",
    "python/peps",
    "python/devguide",
    "Add repository…",
  ]);
  await expect(picker.getByRole("option", { selected: true })).toHaveText("python/peps");
  await page.keyboard.press("Enter");

  await expect(picker).toBeHidden();
  await expect(page.getByRole("button", { name: "python/peps", exact: true })).toBeVisible();
  await expect(page).toHaveTitle("python/peps · Tsugiki 1.2.3");

  // Back again: the core now lists cpython first among the others.
  await changeAnswer(page, "openRepository", ok(openOn(cpython, [peps.name])));
  await page.keyboard.press("ControlOrMeta+R");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("button", { name: "python/cpython", exact: true })).toBeVisible();
});

// The picker hands the focus back to the button as it closes, and Chromium clicks a focused button on the
// keypress after Enter's keydown, which opened the picker again.
test("Enter in the picker opened from the Sidebar's button leaves it closed", async ({ page }) => {
  await mockCore(page, {
    home: openOn(cpython, [peps.name]),
    openRepository: ok(openOn(peps, [cpython.name])),
    workbench,
  });
  await page.goto("/");
  await page.getByRole("button", { name: "python/cpython", exact: true }).click();
  const picker = page.getByRole("dialog", { name: "Repository picker" });
  await expect(picker).toBeVisible();
  await page.keyboard.press("Enter");

  await expect(page.getByRole("button", { name: "python/peps", exact: true })).toBeVisible();
  await expect(picker).toBeHidden();
});
