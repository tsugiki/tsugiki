import { expect, test } from "@playwright/test";
import { emitFromCore, mockCore } from "./mock-core.ts";

const online = { state: "online", login: "octocat" } as const;
const offline = { state: "offline" } as const;
const defaults = { settings: { status_bar: { visible: true } }, problems: [] };
const hidden = { settings: { status_bar: { visible: false } }, problems: [] };

test("the status bar shows who is signed in", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("the status bar shows Offline without the network", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: offline });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Offline: read-only");
});

test("the status bar shows the last login before the core has reached GitHub", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: { state: "connecting_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Connecting…");
});

test("the status bar shows the last login while offline", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: { state: "offline_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Offline: read-only");
});

test("the status bar follows the core's status event", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: { state: "connecting" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Connecting…");
  await emitFromCore(page, "status-changed", online);
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("a refused token shows the core's words", async ({ page }) => {
  const problem = { kind: "unauthorized", message: "GitHub refused the token." } as const;
  await mockCore(page, { settings: defaults, status: { state: "problem", problem } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("GitHub refused the token.");
});

test("settings can hide the status bar, and going offline brings it back", async ({ page }) => {
  await mockCore(page, { settings: hidden, status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("");
  await expect(page.getByRole("status")).toHaveCSS("height", "0px");
  await emitFromCore(page, "status-changed", offline);
  await expect(page.getByRole("status")).toHaveText("Offline: read-only");
});

test("a settings file that doesn't parse is named beside the status", async ({ page }) => {
  const problems = ["settings.toml, line 3: expected a value"];
  await mockCore(page, { settings: { ...defaults, problems }, status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · settings.toml, line 3: expected a value");
});

test("a settings file that doesn't parse brings a hidden status bar back", async ({ page }) => {
  const problems = ["settings.toml, line 3: expected a value"];
  await mockCore(page, { settings: { ...hidden, problems }, status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("settings.toml, line 3: expected a value");
});
