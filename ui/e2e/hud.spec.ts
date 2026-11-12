import { expect, type Page, test } from "@playwright/test";
import type {
  Filter,
  IssueView_Serialize as IssueView,
  ListedItem_Serialize as ListedItem,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, type CoreAnswers, emitFromCore, mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;

const core: CoreAnswers = {
  home: { screen: "repository", repository, others: [] as string[] },
  workbench: { status: "ok", data: { items: [], quiet: [], count: 0, review_requests: 0, filters: [] } },
  timeRunning: 640,
  recordMeasurement: "Performance of Tsugiki 0.1.0 over 1.0 s: frame p95 16.7 ms",
};

const hud = (page: Page) => page.getByRole("complementary", { name: "Performance HUD" });

/** The value on the HUD's line named `name`. */
const value = (page: Page, name: string) =>
  hud(page)
    .getByRole("row", { name: new RegExp(`^${name}`) })
    .getByRole("cell")
    .first();

async function openApp(page: Page): Promise<void> {
  await mockCore(page, core);
  await page.goto("/");
  await expect(page.getByText(/Nothing here yet/)).toBeVisible();
}

test("Ctrl+Shift+H shows the performance HUD, and its numbers move", async ({ page }) => {
  await openApp(page);
  await expect(hud(page)).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+Shift+H");

  await expect(hud(page)).toBeVisible();
  await expect(value(page, "Start-up")).toHaveText("640 ms");
  await expect(value(page, "View query")).toHaveText(/^\d+\.\d ms$/);
  await expect(value(page, "Frame, latest")).toHaveText(/^\d+\.\d ms$/);
  const frames = value(page, "Frames over");
  const counted = await frames.textContent();
  await expect(frames).not.toHaveText(counted ?? "");
  await page.keyboard.press("j");
  await expect(value(page, "Key to frame")).toHaveText(/^\d+\.\d ms$/);

  await page.keyboard.press("ControlOrMeta+Shift+H");
  await expect(hud(page)).toHaveCount(0);
});

test("the palette shows and hides the performance HUD", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Command" }).fill("hud");
  await page.keyboard.press("Enter");
  await expect(hud(page)).toBeVisible();

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("option", { name: "Hide performance HUD" }).click();
  await expect(hud(page)).toHaveCount(0);
});

test("a recording says the line it wrote to the log", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+H");
  const start = hud(page).getByRole("button", { name: "Start recording" });
  await start.click();
  const clicked = await hud(page).getByRole("button", { name: "Stop recording" }).boundingBox();
  await hud(page).getByRole("button", { name: "Stop recording" }).click();
  await expect(hud(page).getByText(/^Written to tsugiki\.log/)).toHaveText(
    "Written to tsugiki.log: Performance of Tsugiki 0.1.0 over 1.0 s: frame p95 16.7 ms",
  );
  // The line goes above the button, so the next click finds the button where the last one did. Its label is a
  // letter longer than Stop's.
  expect(await start.boundingBox()).toMatchObject({ x: clicked?.x, y: clicked?.y });
});

test("the recording button stays put when a line about a long frame shows", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("ControlOrMeta+Shift+H");
  const button = hud(page).getByRole("button", { name: "Start recording" });
  await expect(button).toBeVisible();
  const before = await button.boundingBox();

  // A frame of 150 ms is over the budget, so the HUD adds a line that names it. A HUD that widened for that
  // line moved the button out from under a click (#232). Chromium measures the frame in whole vsyncs, so
  // shorter, and the HUD's own first frame is long too, but not this long. A later long frame can take over
  // the line.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          const end = performance.now() + 150;
          while (performance.now() < end);
          resolve();
        });
      }),
  );
  await expect(value(page, "Frame, worst")).toHaveText(/^\d{3,}\.\d ms$/);
  await expect(hud(page).getByText(/^Last long frame/)).toBeVisible();
  expect(await button.boundingBox()).toEqual(before);
});

test("the HUD times an Issue whose text comes from GitHub", async ({ page }) => {
  const listed: ListedItem & { kind: "issue" } = {
    kind: "issue",
    number: 7,
    title: "Issue 7",
    state: "OPEN",
    people: ["issue-author"],
    updated_at: "2026-11-04T10:00:00Z",
    pull_requests: [],
    attention: null,
  };
  const unread: IssueView = {
    ...listed,
    merged: [],
    merged_count: 0,
    author: "issue-author",
    assignees: [],
    content: null,
  };
  await mockCore(page, {
    ...core,
    workbench: {
      status: "ok",
      data: { items: [listed], quiet: [], count: 0, review_requests: 0, filters: [] },
    },
    openIssue: { status: "ok", data: null },
    issue: { status: "ok", data: unread },
  });
  await page.goto("/");
  const listedButton = page.getByRole("button", { name: "#7 Issue 7" });
  await expect(listedButton).toBeVisible();
  await page.keyboard.press("ControlOrMeta+Shift+H");
  await listedButton.click();

  await expect(value(page, "Issue from the database")).toHaveText(/^\d+\.\d ms$/);
  await expect(value(page, "Issue from GitHub")).toHaveText("waiting");

  const content = { body_html: "<p>It crashes</p>", comments: [], earlier_comments: 0, opened_at: null };
  await changeAnswer(page, "issue", { status: "ok", data: { ...unread, content } });
  await emitFromCore(page, "changed", { repository: repository.name, items: [7] });
  await expect(value(page, "Issue from GitHub")).toHaveText(/^\d+\.\d ms$/);
});
