import { expect, test } from "@playwright/test";
import { emitFromCore, mockCore } from "./mock-core.ts";
import { appearance } from "./themes.ts";

const online = { state: "online", login: "octocat" } as const;
const offline = { state: "offline" } as const;
const releases = { check: true };
const diff = { files: "all" } as const;
const defaults = {
  settings: { status_bar: { visible: true }, releases, appearance, diff, repositories: [] },
  problems: [],
};
const hidden = {
  settings: { status_bar: { visible: false }, releases, appearance, diff, repositories: [] },
  problems: [],
};

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

test("the status bar says when a newer version is out, found before or after the window loaded", async ({
  page,
}) => {
  const page_of = (version: string) => `https://github.com/tsugiki/tsugiki/releases/tag/v${version}`;
  await mockCore(page, {
    settings: defaults,
    status: online,
    newVersion: { version: "0.3.0", page: page_of("0.3.0") },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Tsugiki 0.3.0 is out" })).toBeVisible();
  await emitFromCore(page, "new-version-found", { version: "0.4.0", page: page_of("0.4.0") });
  await expect(page.getByRole("button", { name: "Tsugiki 0.4.0 is out" })).toBeVisible();
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

test("a hidden status bar says git is missing until the core finds it", async ({ page }) => {
  await mockCore(page, { settings: hidden, status: online, mirrorProblem: { kind: "no_git" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText(
    "The Files tab needs git 2.30 or newer, and Tsugiki can't find git",
  );
  await page.getByRole("button", { name: "Check again" }).click();
  await emitFromCore(page, "mirror-problem-changed", null);
  await expect(page.getByRole("button", { name: "Check again" })).toHaveCount(0);
  await expect(page.getByRole("status")).toHaveText("");
});

test("a hidden status bar says a mirror waits for an answer, and its dialog says why a clone can't lend and takes Download", async ({
  page,
}) => {
  await mockCore(page, {
    settings: hidden,
    status: online,
    mirrorQuestion: "python/cpython",
    mirrorCost: { status: "ok", data: { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 } },
    pickFolder: { status: "ok", data: "D:/somewhere" },
    checkClone: { status: "ok", data: { kind: "no_remote" } },
    chooseMirrorSource: { status: "ok", data: null },
  });
  await page.goto("/");
  const line = page.getByRole("button", { name: "python/cpython's mirror waits for your answer" });
  await expect(page.getByRole("status")).toHaveText("python/cpython's mirror waits for your answer");
  await expect(page.getByRole("button")).toHaveCount(1);

  await line.click();
  const dialog = page.getByRole("dialog", { name: "Mirror of python/cpython" });
  const start = dialog.getByRole("button", { name: "Start the clone" });
  await dialog.getByRole("radio", { name: "Borrow objects from a clone you have, about 1 minute" }).check();
  await expect(dialog).toContainText("D:/somewhere has no remote for python/cpython.");
  await expect(start).toBeDisabled();
  await dialog.getByRole("radio", { name: "Download it from GitHub, about 3 minutes" }).check();
  await start.click();
  await expect(dialog).toHaveCount(0);
  await emitFromCore(page, "mirror-question-changed", null);

  await expect(line).toHaveCount(0);
  await expect(page.getByRole("status")).toHaveText("");
});
