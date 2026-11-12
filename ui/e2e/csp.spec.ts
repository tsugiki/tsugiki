import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";
import { mockCore } from "./mock-core.ts";
import { appearance } from "./themes.ts";

// The CSP in the Tauri config, as Tauri sends it with the page. Tauri adds each inline script's hash to
// `script-src`, and gives each inline `<style>` a nonce in `style-src` unless the config says not to. A nonce
// there would make the browser ignore the policy's 'unsafe-inline', and refuse each `style` attribute: the
// one on app.html's wrapper, which keeps the status bar at the window's foot, and SvelteKit's own.
const config = JSON.parse(readFileSync("../crates/app/tauri.conf.json", "utf8")) as {
  app: { security: { csp: string; dangerousDisableAssetCspModification?: boolean | string[] } };
};
const { csp: policy, dangerousDisableAssetCspModification: untouched = false } = config.app.security;
const modifies = (directive: string) =>
  Array.isArray(untouched) ? !untouched.includes(directive) : !untouched;

const sha256 = (text: string) => `'sha256-${createHash("sha256").update(text).digest("base64")}'`;

/** Serves the page under the CSP Tauri builds from the config. */
const serveUnderTauriCsp = (page: Page) =>
  page.route("**/*", async (route) => {
    if (route.request().resourceType() !== "document") {
      await route.continue();
      return;
    }
    const response = await route.fetch();
    let html = await response.text();
    let csp = policy;
    if (modifies("script-src")) {
      const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(([, body]) =>
        sha256(body ?? ""),
      );
      csp = csp.replace("script-src 'self'", `script-src 'self' ${scripts.join(" ")}`);
    }
    if (modifies("style-src")) {
      const nonce = randomBytes(16).toString("base64");
      html = html.replaceAll("<style>", `<style nonce="${nonce}">`);
      csp = csp.replace("style-src 'self'", `style-src 'self' 'nonce-${nonce}'`);
    }
    await route.fulfill({
      response,
      body: html,
      headers: { ...response.headers(), "content-security-policy": csp },
    });
  });

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
