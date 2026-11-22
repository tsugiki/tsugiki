import { expect, test } from "@playwright/test";
import { done, loadedSettings, ok, workbenchPage } from "../src/lib/testing/fixtures.ts";
import { home, issue, issueView } from "./fixtures.ts";
import { emitFromCore, mockCore } from "./mock-core.ts";

const online = { state: "online", login: "octocat" } as const;
const offline = { state: "offline" } as const;
const defaults = loadedSettings();
const hidden = loadedSettings({ status_bar: { visible: false } });

test("the status bar shows who is signed in", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: online });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat");
});

test("the status bar shows Offline without the network", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: offline });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("Offline");
});

test("the status bar shows the last login before the core has reached GitHub", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: { state: "connecting_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Connecting…");
});

test("the status bar shows the last login while offline", async ({ page }) => {
  await mockCore(page, { settings: defaults, status: { state: "offline_as", last_login: "octocat" } });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Offline");
});

test("a Read-only Repository says so in the status bar and atop its Issue view, and Offline apart", async ({
  page,
}) => {
  await mockCore(page, {
    settings: defaults,
    status: { state: "offline_as", last_login: "octocat" },
    home,
    workbench: workbenchPage([issue]),
    openIssue: done,
    issue: ok(issueView),
    repositoryState: ok({ read_only: ["token"], connection: "offline" }),
  });
  await page.goto("/");
  await expect(page.getByRole("status")).toHaveText("octocat · Offline · Read-only");

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const view = page.getByRole("region", { name: "Issue view" });
  await expect(view.getByText("Read-only: your token can't write to python/cpython")).toBeVisible();
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

const longAgo = () => new Date(Date.now() - 60_000).toISOString();
const longSubject = {
  asked: { kind: "refresh", repository: `some-maintainer/${"g".repeat(80)}` },
  since: longAgo(),
} as const;

test("a slow request keeps one width whatever it asks, and a long subject ends in an ellipsis before the seconds", async ({
  page,
}) => {
  const short = { asked: { kind: "status" }, since: longAgo() } as const;
  await mockCore(page, { settings: defaults, status: online, inFlight: short });
  await page.goto("/");
  const part = page.getByTitle(/^Waiting for GitHub/);
  await expect(part).toHaveText(/^Waiting for GitHub: status check, \d+ s$/);
  const shortBox = await part.boundingBox();

  await emitFromCore(page, "in-flight-changed", longSubject);
  await expect(part).toHaveAttribute(
    "title",
    /^Waiting for GitHub: refresh of some-maintainer\/g{80}, \d+ s$/,
  );
  const longBox = await part.boundingBox();
  const subject = part.getByText(/^Waiting for GitHub/);
  const cut = await subject.evaluate((element) => element.scrollWidth > element.clientWidth);
  const seconds = await part.getByText(/^, \d+ s$/).boundingBox();
  await emitFromCore(page, "in-flight-changed", null);

  expect(shortBox).not.toBeNull();
  expect(longBox).not.toBeNull();
  expect(Math.abs((longBox?.width ?? 0) - (shortBox?.width ?? 0))).toBeLessThanOrEqual(0.5);
  expect(Math.abs((longBox?.x ?? 0) - (shortBox?.x ?? 0))).toBeLessThanOrEqual(0.5);
  expect(cut).toBe(true);
  // The seconds sit whole inside the part, at its end.
  expect(seconds?.width ?? 0).toBeGreaterThan(0);
  const partEnd = (longBox?.x ?? 0) + (longBox?.width ?? 0);
  expect((seconds?.x ?? 0) + (seconds?.width ?? 0)).toBeLessThanOrEqual(partEnd + 0.5);
  await expect(part).toHaveCount(0);
});

test("in a narrow window a slow request leaves the status room", async ({ page }) => {
  await page.setViewportSize({ width: 700, height: 500 });
  await mockCore(page, { settings: defaults, status: online, inFlight: longSubject });
  await page.goto("/");
  const part = page.getByTitle(/^Waiting for GitHub/);
  await expect(part).toBeVisible();

  const status = await page.getByRole("status").boundingBox();
  const partBox = await part.boundingBox();

  expect(status?.width ?? 0).toBeGreaterThan(200);
  expect(partBox?.width ?? 0).toBeLessThanOrEqual(0.3 * 700 + 16 + 0.5);
  await expect(part.getByText(/^, \d+ s$/)).toBeVisible();
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
  await expect(page.getByRole("status")).toHaveText("Offline");
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
    mirrorCost: ok({ mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 }),
    pickFolder: ok("D:/somewhere"),
    checkClone: ok({ kind: "no_remote" }),
    chooseMirrorSource: done,
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
