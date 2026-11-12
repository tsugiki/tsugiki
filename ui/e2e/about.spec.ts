import { expect, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";

test("the window shows what the core says about itself", async ({ page }) => {
  await mockCore(page, { about: { name: "Tsugiki", version: "1.2.3", build: "release" } });
  await page.goto("/");
  await expect(page.getByText("Tsugiki 1.2.3")).toBeVisible();
});

test("the window says when the core doesn't answer", async ({ page }) => {
  await mockCore(page, {});
  await page.goto("/");
  await expect(page.getByText("The core didn't answer")).toBeVisible();
});
