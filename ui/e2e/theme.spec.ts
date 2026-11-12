import { expect, type Page, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";
import { themes } from "./themes.ts";

const colours = (page: Page) =>
  page.evaluate(() => {
    const body = getComputedStyle(document.body);
    return {
      background: body.backgroundColor,
      text: body.color,
      scheme: getComputedStyle(document.documentElement).colorScheme,
    };
  });

test("the window takes the theme of the side the system is on, and turns with it", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, { themes: { status: "ok", data: themes } });
  await page.goto("/");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(255, 255, 255)", text: "rgb(59, 59, 59)", scheme: "light" });

  await page.emulateMedia({ colorScheme: "dark" });
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(31, 31, 31)", text: "rgb(204, 204, 204)", scheme: "dark" });
});

test("the theme picker previews a theme on the window, and Esc puts the old one back", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await mockCore(page, { themes: { status: "ok", data: themes } });
  await page.goto("/");
  const darkModern = { background: "rgb(31, 31, 31)", text: "rgb(204, 204, 204)", scheme: "dark" };
  await expect.poll(() => colours(page)).toEqual(darkModern);

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("option", { name: "Pick a Theme" }).click();
  const box = page.getByRole("combobox", { name: "Theme" });
  await box.press("ArrowDown");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(29, 32, 33)", text: "rgb(235, 219, 178)", scheme: "dark" });
  await box.press("Escape");

  await expect(page.getByRole("dialog", { name: "Theme picker" })).toBeHidden();
  await expect.poll(() => colours(page)).toEqual(darkModern);
});

test("Light mode keeps the light theme while the system is dark", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await mockCore(page, { themes: { status: "ok", data: { ...themes, mode: "light" } } });
  await page.goto("/");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(255, 255, 255)", text: "rgb(59, 59, 59)", scheme: "light" });
});
