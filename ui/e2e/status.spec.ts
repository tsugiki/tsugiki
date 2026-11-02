import { expect, test } from "@playwright/test";
import { emitFromCore, mockCore } from "./mock-core.ts";

const online = { state: "online", login: "octocat" } as const;
const offline = { state: "offline" } as const;

test("the status bar shows who is signed in", async ({ page }) => {
  await mockCore(page, { status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("the status bar shows Offline without the network", async ({ page }) => {
  await mockCore(page, { status: offline });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Offline: read-only");
});

test("the status bar shows the last login before the core has reached GitHub", async ({ page }) => {
  await mockCore(page, { status: { state: "connecting_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Connecting…");
});

test("the status bar shows the last login while offline", async ({ page }) => {
  await mockCore(page, { status: { state: "offline_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Offline: read-only");
});

test("the status bar follows the core's status event", async ({ page }) => {
  await mockCore(page, { status: { state: "connecting" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Connecting…");
  await emitFromCore(page, "status-changed", online);
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("a refused token shows the core's words", async ({ page }) => {
  const problem = { kind: "unauthorized", message: "GitHub refused the token." } as const;
  await mockCore(page, { status: { state: "problem", problem } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("GitHub refused the token.");
});
