import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";

// The CSP in the Tauri config, as Tauri sends it with the page. Tauri lets the page's inline scripts run
// through `script-src`, and gives each inline `<style>` a nonce in `style-src` unless the config says not to.
// A nonce there would make the browser ignore the policy's 'unsafe-inline', and refuse each `style`
// attribute: the one on app.html's wrapper, which keeps the status bar at the window's foot, and SvelteKit's.
const config = JSON.parse(readFileSync("../crates/app/tauri.conf.json", "utf8")) as {
  app: { security: { csp: string; dangerousDisableAssetCspModification?: boolean | string[] } };
};
const { csp: policy, dangerousDisableAssetCspModification: untouched = false } = config.app.security;
const modifies = (directive: string) =>
  Array.isArray(untouched) ? !untouched.includes(directive) : !untouched;

/** Gives each `<tag>` in `html` a fresh nonce, and adds it to `directive` in `csp`. */
const allowInline = (html: string, csp: string, tag: string, directive: string) => {
  const nonce = randomBytes(16).toString("base64");
  return {
    html: html.replaceAll(`<${tag}>`, `<${tag} nonce="${nonce}">`),
    csp: csp.replace(`${directive} 'self'`, `${directive} 'self' 'nonce-${nonce}'`),
  };
};

/** Serves the page with the CSP header Tauri builds from the config. The test server sends none. */
export const serveUnderTauriCsp = (page: Page) =>
  page.route("/", async (route) => {
    const response = await route.fetch();
    let served = { html: await response.text(), csp: policy };
    // Tauri adds the scripts' hashes, and a nonce lets the same scripts run.
    if (modifies("script-src")) served = allowInline(served.html, served.csp, "script", "script-src");
    if (modifies("style-src")) served = allowInline(served.html, served.csp, "style", "style-src");
    // The nonces make the page longer than the length the server gave.
    const headers: Record<string, string> = {
      ...response.headers(),
      "content-security-policy": served.csp,
    };
    delete headers["content-length"];
    await route.fulfill({ response, body: served.html, headers });
  });
