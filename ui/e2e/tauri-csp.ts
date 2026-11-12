import { createHash, randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import type { Page } from "@playwright/test";

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

/** Serves the page with the CSP header Tauri builds from the config. The test server sends none. */
export const serveUnderTauriCsp = (page: Page) =>
  page.route("/", async (route) => {
    const response = await route.fetch();
    let html = await response.text();
    let csp = policy;
    if (modifies("script-src")) {
      // The browser hashes a script with its line ends turned to LF, and so does Tauri.
      const scripts = [...html.matchAll(/<script>([\s\S]*?)<\/script>/g)].map(([, body]) =>
        sha256((body ?? "").replace(/\r\n?/g, "\n")),
      );
      csp = csp.replace("script-src 'self'", `script-src 'self' ${scripts.join(" ")}`);
    }
    if (modifies("style-src")) {
      const nonce = randomBytes(16).toString("base64");
      html = html.replaceAll("<style>", `<style nonce="${nonce}">`);
      csp = csp.replace("style-src 'self'", `style-src 'self' 'nonce-${nonce}'`);
    }
    // The nonces make the page longer than the length the server gave.
    const headers: Record<string, string> = { ...response.headers(), "content-security-policy": csp };
    delete headers["content-length"];
    await route.fulfill({ response, body: html, headers });
  });
