import { expect, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";
import { serveUnderTauriCsp } from "./tauri-csp.ts";
import { appearance } from "./themes.ts";

const settings = {
  settings: {
    status_bar: { visible: true },
    releases: { check: true },
    appearance,
    diff: { files: "all" as const },
    repositories: [],
  },
  problems: [],
};

test("the window keeps the status bar at its foot under Tauri's CSP, which the page breaks no rule of", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const seen: string[] = [];
    Object.assign(window, { cspViolations: seen });
    document.addEventListener("securitypolicyviolation", (e) => {
      seen.push(`${e.violatedDirective} ${e.sourceFile}:${String(e.lineNumber)}`);
    });
  });
  await serveUnderTauriCsp(page);
  await mockCore(page, { settings, status: { state: "online", login: "octocat" } });
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto("/");

  const bar = page.getByRole("status");
  await expect(bar).toHaveText("octocat");
  expect(await page.evaluate(() => (window as unknown as { cspViolations: string[] }).cspViolations)).toEqual(
    [],
  );
  const box = await bar.boundingBox();
  expect((box?.y ?? 0) + (box?.height ?? 0)).toBe(600);
});
