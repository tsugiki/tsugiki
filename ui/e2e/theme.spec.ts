import { expect, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";

/** Two themes cut down to the tokens the test reads. */
const themes = {
  light: {
    name: "Light",
    kind: "light" as const,
    tokens: [
      { property: "--bg", value: "#ffffff" },
      { property: "--fg", value: "#3b3b3b" },
    ],
  },
  dark: {
    name: "Dark",
    kind: "dark" as const,
    tokens: [
      { property: "--bg", value: "#1f1f1f" },
      { property: "--fg", value: "#cccccc" },
    ],
  },
};

const colours = (page: import("@playwright/test").Page) =>
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
