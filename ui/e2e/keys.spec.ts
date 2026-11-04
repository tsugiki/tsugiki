import { expect, type Page, test } from "@playwright/test";
import { type CoreAnswers, mockCore } from "./mock-core.ts";

const core: CoreAnswers = {
  settings: { settings: { status_bar: { visible: true }, repositories: [] }, problems: [] },
  status: { state: "online", login: "octocat" },
};

/** A key pressed before the page has mounted reaches no guard, so wait for the page to say something. */
async function openApp(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat");
}

test("Ctrl+Shift+P opens the command palette, and Esc closes it", async ({ page }) => {
  await mockCore(page, core);
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  const command = page.getByRole("combobox", { name: "Command" });
  await expect(command).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("a click outside the command palette closes it", async ({ page }) => {
  await mockCore(page, core);
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByTestId("palette-scrim").click({ position: { x: 5, y: 5 } });
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("the keys Tsugiki owns stop before the browser, on a Russian layout too", async ({ page }) => {
  await mockCore(page, core);
  await openApp(page);
  const stopped = await page.evaluate(() => {
    const primary = navigator.platform.startsWith("Mac") ? { metaKey: true } : { ctrlKey: true };
    const press = (init: KeyboardEventInit) => {
      const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...primary, ...init });
      document.body.dispatchEvent(event);
      return event.defaultPrevented;
    };
    return {
      latin: press({ key: "p", code: "KeyP" }),
      russian: press({ key: "з", code: "KeyP" }),
      notOurs: press({ key: "c", code: "KeyC" }),
    };
  });
  expect(stopped).toEqual({ latin: true, russian: true, notOurs: false });
});
