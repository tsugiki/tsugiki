import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import type {
  Filter,
  IssueView_Serialize as IssueView,
  ListedItem_Serialize as ListedItem,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const cpython = { screen: "repository", repository } as const;

const row: Omit<WorkbenchRow, "number"> = {
  title: "gh-7: Fix it",
  author: "some-author",
  state: "MERGED",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: "APPROVED",
};

const issue = (number: number, waiting: boolean): ListedItem => ({
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [],
  attention: waiting
    ? { since: "2026-11-04T10:00:00Z", why: [{ kind: "comment", by: "some-reviewer" }] }
    : null,
});

const listing = (waiting: boolean) => ({
  status: "ok" as const,
  data: {
    items: [issue(7, waiting), issue(8, false)],
    quiet: [],
    count: waiting ? 1 : 0,
    review_requests: 0,
    filters: [],
  },
});

const view = (bodyHtml: string): IssueView => ({
  number: 7,
  title: "Issue 7",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  author: "issue-author",
  pull_requests: [
    {
      kind: "pr",
      number: 10,
      ...row,
      backports: [{ number: 11, ...row, target_branch: "3.14", backport: "bot" }],
    },
  ],
  content: {
    body_html: bodyHtml,
    comments: [
      {
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: "<p>Which build?</p>",
        hidden: null,
      },
    ],
    earlier_comments: 0,
  },
});

const held = (bodyHtml: string) => ({ status: "ok", data: view(bodyHtml) }) as const;
const done = { status: "ok", data: null } as const;
const answers = (bodyHtml: string) => ({
  home: cpython,
  workbench: listing(true),
  openIssue: done,
  fetchIssue: done,
  openOnGithub: done,
  openLink: done,
  issue: held(bodyHtml),
});

test("an Issue opens in the Detail pane and stops looking unread", async ({ page }) => {
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");

  await page.getByRole("button", { name: "#7 Issue 7" }).click();

  const pane = page.getByRole("region", { name: "Issue view" });
  await expect(pane.getByRole("heading")).toHaveText("#7 Issue 7");
  await expect(pane).toContainText("issue-author");
  await expect(pane).toContainText("It crashes");
  await expect(pane).toContainText("Which build?");
  await expect(pane).toContainText("Merged · CI passed · You approved");
  await expect(pane).toContainText("3.14 Merged");

  await changeAnswer(page, "workbench", listing(false));
  await emitFromCore(page, "changed", { repository: "python/cpython", items: [7] });
  await expect(page.getByRole("button", { name: "#7 Issue 7" }).locator("strong")).toHaveCount(0);
});

test("the arrows move through the list and Enter opens the selection", async ({ page }) => {
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "#8 Issue 8" })).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await expect(page.getByRole("region", { name: "Issue view" })).toHaveCount(0);

  await page.keyboard.press("Enter");

  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("It crashes");
});

test("an Issue opened far down a long list shows beside it", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 400 });
  const numbers = Array.from({ length: 40 }, (_, at) => 100 + at);
  await mockCore(page, {
    ...answers("<p>It crashes</p>"),
    workbench: {
      status: "ok",
      data: {
        items: numbers.map((number) => issue(number, false)),
        quiet: [],
        count: 0,
        review_requests: 0,
        filters: [],
      },
    },
  });
  await page.goto("/");

  await page.getByRole("button", { name: "#139 Issue 139" }).click();

  await expect(page.getByRole("region", { name: "Issue view" }).getByRole("heading")).toBeInViewport();
});

test("a link in a description leaves the window where it is", async ({ page }) => {
  await mockCore(page, answers('<p><a href="https://github.com/python/cpython">the code</a></p>'));
  await page.goto("/");
  const address = page.url();

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  await page.getByRole("link", { name: "the code" }).click();

  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("the code");
  expect(page.url()).toBe(address);
});

test("a refresh or a base address in a description leaves the window where it is", async ({ page }) => {
  await mockCore(
    page,
    answers(
      '<meta http-equiv="refresh" content="0; url=/?moved">' +
        '<base href="https://github.com/elsewhere/"><p>Read this</p>',
    ),
  );
  await page.goto("/");
  const address = page.url();

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("Read this");
  // A refresh of 0 seconds would have moved the page by the time it settles.
  await page.waitForLoadState("networkidle");

  expect(page.url()).toBe(address);
  expect(await page.evaluate(() => document.baseURI)).toBe(address);
});

test("the content security policy stops what a description tries to run or load", async ({ page }) => {
  const config = JSON.parse(readFileSync("../crates/app/tauri.conf.json", "utf8")) as {
    app: { security: { csp: string } };
  };
  await mockCore(
    page,
    answers(
      '<img alt="handler" src="data:," onerror="window.ranFromDescription = true">' +
        '<img alt="remote" src="https://github.com/favicon.ico">',
    ),
  );
  const asked: string[] = [];
  page.on("request", (request) => {
    if (request.url().startsWith("https://")) asked.push(request.url());
  });
  await page.goto("/");
  // Tauri sends this policy as a header. The test server has none, and a tag applies from here on.
  await page.evaluate((policy) => {
    const tag = document.createElement("meta");
    tag.httpEquiv = "Content-Security-Policy";
    tag.content = policy;
    document.head.append(tag);
  }, config.app.security.csp);

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const remote = page.getByAltText("remote");
  await expect(remote).toBeAttached();
  await expect(page.getByAltText("handler")).toBeAttached();
  // Each image has had its chance to fail by the time the page settles.
  await page.waitForLoadState("networkidle");

  expect(await page.evaluate(() => "ranFromDescription" in window)).toBe(false);
  expect(asked).toEqual([]);
});

test("the edges between the panes drag, a double-click resets one, and the list gives way first", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  const list = page.locator(".list");
  const pane = page.getByRole("region", { name: "Issue view" });
  const width = async (part: typeof list) => (await part.boundingBox())?.width ?? 0;
  const drag = async (name: string, by: number) => {
    const edge = (await page.getByRole("separator", { name }).boundingBox()) ?? { x: 0, y: 0 };
    await page.mouse.move(edge.x + 8, edge.y + 20);
    await page.mouse.down();
    await page.mouse.move(edge.x + 8 + by, edge.y + 20, { steps: 4 });
    await page.mouse.up();
  };
  await drag("Sidebar width", 60);
  const even = await width(list);

  await drag("Workbench width", 100);
  const dragged = [await width(sidebar), await width(list)];
  await page.setViewportSize({ width: 900, height: 600 });
  const narrow = [await width(list), await width(pane)];
  await page.setViewportSize({ width: 1200, height: 600 });
  await page.getByRole("separator", { name: "Workbench width" }).dblclick();

  expect(dragged).toEqual([260, Math.round(even) + 100]);
  expect(narrow[0]).toBeLessThan(dragged[1] ?? 0);
  expect(narrow[1]).toBeGreaterThanOrEqual(320);
  expect(Math.abs((await width(list)) - (await width(pane)))).toBeLessThan(2);
});
