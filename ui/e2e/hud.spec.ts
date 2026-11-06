import { expect, type Page, test } from "@playwright/test";
import type { Filter } from "../src/lib/data/bindings.ts";
import { type CoreAnswers, mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;

const core: CoreAnswers = {
  home: { screen: "repository", repository },
  workbench: { status: "ok", data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] } },
  timeRunning: 640,
  recordMeasurement: "Performance of Tsugiki 0.1.0 over 1.0 s: frame p95 16.7 ms",
};

const hud = (page: Page) => page.getByRole("complementary", { name: "Performance HUD" });

/** The value on the HUD's line named `name`. */
const value = (page: Page, name: string) =>
  hud(page)
    .getByRole("row", { name: new RegExp(`^${name}`) })
    .getByRole("cell")
    .first();

async function openApp(page: Page): Promise<void> {
  await mockCore(page, core);
  await page.goto("/");
  await expect(page.getByText(/Nothing here yet/)).toBeVisible();
}

test("Ctrl+Shift+H shows the performance HUD, and its numbers move", async ({ page }) => {
  await openApp(page);
  await expect(hud(page)).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+Shift+H");

  await expect(hud(page)).toBeVisible();
  await expect(value(page, "Start-up")).toHaveText("640 ms");
  await expect(value(page, "View query")).toHaveText(/^\d+\.\d ms$/);
  await expect(value(page, "Frame, latest")).toHaveText(/^\d+\.\d ms$/);
  const frames = value(page, "Frames over");
  const counted = await frames.textContent();
  await expect(frames).not.toHaveText(counted ?? "");
  await page.keyboard.press("j");
  await expect(value(page, "Key to frame")).toHaveText(/^\d+\.\d ms$/);

  await page.keyboard.press("ControlOrMeta+Shift+H");
  await expect(hud(page)).toHaveCount(0);
});

test("the palette shows and hides the performance HUD", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Command" }).fill("hud");
  await page.keyboard.press("Enter");
  await expect(hud(page)).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("option", { name: "Hide performance HUD" }).click();
  await expect(hud(page)).toHaveCount(0);
});

test("a recording says the line it wrote to the log", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+H");
  await hud(page).getByRole("button", { name: "Start recording" }).click();
  await hud(page).getByRole("button", { name: "Stop recording" }).click();
  await expect(hud(page).getByText(/^Written to tsugiki\.log/)).toHaveText(
    "Written to tsugiki.log: Performance of Tsugiki 0.1.0 over 1.0 s: frame p95 16.7 ms",
  );
});
