import { expect, test } from "@playwright/test";
import { workbenchPage } from "../src/lib/testing/fixtures.ts";
import { home } from "./fixtures.ts";
import { mockCore } from "./mock-core.ts";

const about = {
  name: "Tsugiki",
  version: "1.2.3 (abc1234)",
  build: "release",
  source_ref: "v1.2.3",
} as const;

test("the window's title names the build, and the screen doesn't", async ({ page }) => {
  await mockCore(page, { about });
  await page.goto("/");
  await expect(page).toHaveTitle("Tsugiki 1.2.3 (abc1234)");
  await expect(page.getByText("1.2.3")).toHaveCount(0);
});

test("the window's title names the open Repository before the build", async ({ page }) => {
  await mockCore(page, {
    about,
    home,
    workbench: workbenchPage([]),
  });
  await page.goto("/");
  await expect(page).toHaveTitle("python/cpython · Tsugiki 1.2.3 (abc1234)");
});

test("the status bar says when the core doesn't answer", async ({ page }) => {
  await mockCore(page, {}, ["about"]);
  await page.goto("/");
  await expect(page.getByRole("status")).toContainText("The core didn't answer");
  await expect(page).toHaveTitle("Tsugiki");
});
