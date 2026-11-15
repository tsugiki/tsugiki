import { expect, type Page, test } from "@playwright/test";
import type { LoadedSettings_Serialize } from "../src/lib/data/bindings.ts";
import { mockCore, noServers } from "./mock-core.ts";
import { appearance, themes } from "./themes.ts";

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
  await expect(page.getByRole("option", { selected: true })).toHaveText(/Follow system/);
  await box.press("ArrowDown");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(255, 255, 255)", text: "rgb(59, 59, 59)", scheme: "light" });
  await box.press("ArrowDown");
  await box.press("ArrowDown");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(29, 32, 33)", text: "rgb(235, 219, 178)", scheme: "dark" });
  await box.press("Escape");

  await expect(page.getByRole("dialog", { name: "Theme picker" })).toBeHidden();
  await expect.poll(() => colours(page)).toEqual(darkModern);
});

test("A light theme the settings name shows while the system is dark", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  const theme = { kind: "named", name: themes.light.chosen, side: "light" } as const;
  await mockCore(page, { themes: { status: "ok", data: { ...themes, theme } } });
  await page.goto("/");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(255, 255, 255)", text: "rgb(59, 59, 59)", scheme: "light" });
});

test("Settings › General shows a dark Theme on the window before Apply, and Cancel puts the light one back", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  const loaded: LoadedSettings_Serialize = {
    settings: {
      status_bar: { visible: true },
      releases: { check: true },
      appearance,
      diff: { files: "all", peek: "inline" },
      keys: {},
      repositories: [],
    },
    language_servers: noServers,
    problems: [],
  };
  await mockCore(page, {
    themes: { status: "ok", data: themes },
    settings: loaded,
    rereadSettings: { status: "ok", data: loaded },
  });
  await page.goto("/");
  const lightModern = { background: "rgb(255, 255, 255)", text: "rgb(59, 59, 59)", scheme: "light" };
  await expect.poll(() => colours(page)).toEqual(lightModern);

  await page.keyboard.press("ControlOrMeta+,");
  const theme = page.getByRole("combobox", { name: "Theme", exact: true });
  await expect(theme).toBeEnabled();
  await theme.selectOption("Gruvbox Dark Hard");
  await expect
    .poll(() => colours(page))
    .toEqual({ background: "rgb(29, 32, 33)", text: "rgb(235, 219, 178)", scheme: "dark" });

  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("dialog", { name: "Settings" })).toBeHidden();
  await expect.poll(() => colours(page)).toEqual(lightModern);
});
