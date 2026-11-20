import { expect, test } from "@playwright/test";
import { loadedSettings } from "../src/lib/testing/fixtures.ts";
import { mockCore } from "./mock-core.ts";
import { cspViolations, serveUnderTauriCsp, watchCspViolations } from "./tauri-csp.ts";

const settings = loadedSettings();

test("the window keeps the status bar at its foot under Tauri's CSP, which the page breaks no rule of", async ({
  page,
}) => {
  await watchCspViolations(page);
  await serveUnderTauriCsp(page);
  await mockCore(page, { settings, status: { state: "online", login: "octocat" } });
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/");

  const bar = page.getByRole("status");
  await expect(bar).toHaveText("octocat");
  expect(await cspViolations(page)).toEqual([]);
  const box = await bar.boundingBox();
  expect((box?.y ?? 0) + (box?.height ?? 0)).toBe(600);
});
