import { expect, test } from "@playwright/test";
import { emitFromCore, mockCore } from "./mock-core.ts";

const online = { connection: "online", login: "octocat", problem: null } as const;
const offline = { connection: "offline", login: null, problem: null } as const;

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

test("the status bar follows the core's status event", async ({ page }) => {
  await mockCore(page, { status: { connection: "unknown", login: null, problem: null } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Connecting…");
  await emitFromCore(page, "status-changed", online);
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("a refused token shows the core's words", async ({ page }) => {
  const problem = { kind: "unauthorized", message: "GitHub refused the token.", resets_at: null } as const;
  await mockCore(page, { status: { connection: "online", login: null, problem } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("GitHub refused the token.");
});
