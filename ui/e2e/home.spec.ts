import { expect, type Page, test } from "@playwright/test";
import type { Filter_Serialize as Filter } from "../src/lib/data/bindings.ts";
import { ok, openOn, workbenchPage } from "../src/lib/testing/fixtures.ts";
import { palette, repository as cpython } from "./fixtures.ts";
import { mockCore } from "./mock-core.ts";

const peps = { name: "python/peps", conventions: "github", filters: [] as Filter[] } as const;
const HOUR_MS = 3_600_000;
/** A time `hours` before now, as the core writes it. */
const hoursAgo = (hours: number) => new Date(Date.now() - hours * HOUR_MS).toISOString();

test.beforeEach(async ({ page }) => {
  await mockCore(page, {
    home: openOn(cpython, [{ name: peps.name, opened_at: hoursAgo(3) }], hoursAgo(1)),
    openRepository: ok(openOn(peps, [cpython.name])),
    workbench: workbenchPage([]),
  });
  await page.goto("/");
  await expect(sidebarName(page, cpython.name)).toBeVisible();
});

const sidebarName = (page: Page, name: string) => page.getByRole("button", { name, exact: true });
const rows = (page: Page) => page.getByRole("list", { name: "Repositories" }).getByRole("button");

test("goes to Home from the picker's first row, and back to the Repository on Enter", async ({ page }) => {
  await page.keyboard.press("ControlOrMeta+R");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");

  await expect(page.getByRole("heading", { name: "Home" })).toBeVisible();
  await expect(rows(page)).toHaveText([
    /^python\/cpython\s*opened 1 h ago$/,
    /^python\/peps\s*opened 3 h ago$/,
  ]);
  await expect(rows(page).first()).toBeFocused();
  await expect(page.getByRole("status")).toBeVisible();
  await expect(page).toHaveTitle("Home · Tsugiki 1.2.3");

  await page.keyboard.press("Enter");
  await expect(sidebarName(page, cpython.name)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Home" })).toBeHidden();
  await expect(page).toHaveTitle("python/cpython · Tsugiki 1.2.3");
});

test("goes to Home from the palette, and back on Esc and on Alt+←", async ({ page }) => {
  await palette(page, "Go to Home");
  await expect(page.getByRole("heading", { name: "Home" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(sidebarName(page, cpython.name)).toBeVisible();

  await palette(page, "Go to Home");
  await expect(rows(page).first()).toBeFocused();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(sidebarName(page, cpython.name)).toBeVisible();
});

test("opens another Repository on its Workbench with ↓ and Enter", async ({ page }) => {
  await palette(page, "Go to Home");
  await expect(rows(page).first()).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(rows(page).nth(1)).toBeFocused();
  await page.keyboard.press("Enter");

  await expect(sidebarName(page, peps.name)).toBeVisible();
  await expect(page).toHaveTitle("python/peps · Tsugiki 1.2.3");
});

test("switches from the palette's Repository: row on Home", async ({ page }) => {
  await palette(page, "Go to Home");
  await expect(rows(page).first()).toBeFocused();
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Command" }).fill("Repository: python/peps");
  await page.keyboard.press("Enter");
  await expect(sidebarName(page, peps.name)).toBeVisible();
});
