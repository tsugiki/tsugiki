import { readFileSync } from "node:fs";
import { expect, type Locator, type Page, test } from "@playwright/test";
import type {
  Filter,
  IssueView_Serialize as IssueView,
  ListedItem_Serialize as ListedItem,
  TreeEntry_Serialize as TreeEntry,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";
import { themes } from "./themes.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;
const cpython = { screen: "repository", repository, others: [] as string[] } as const;

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
  seen: { mark: { numbers: [], up_to: null }, state: "seen" },
};

const issue = (number: number, waiting: boolean, pullRequests: TreeEntry[] = []): ListedItem => ({
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: pullRequests,
  seen: { mark: { numbers: [number], up_to: null }, state: "seen" },
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
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  author: "issue-author",
  assignees: ["some-assignee"],
  labels: ["type-bug"],
  needs_attention: [10],
  seen: { mark: { numbers: [7, 10], up_to: null }, state: "seen" },
  pull_requests: [
    {
      kind: "pr",
      number: 10,
      ...row,
      backports: [{ number: 11, ...row, target_branch: "3.14", backport: "bot" }],
    },
  ],
  merged: [],
  merged_count: 0,
  content: {
    body_html: bodyHtml,
    comments: [
      {
        id: 1,
        author: "some-reviewer",
        created_at: "2026-11-04T09:00:00Z",
        body_html: "<p>Which build?</p>",
        hidden: null,
      },
    ],
    earlier_comments: 0,
    opened_at: "2026-11-01T09:00:00Z",
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
  await expect(pane.getByRole("heading", { level: 2 })).toHaveText("#7 Issue 7");
  await expect(pane.getByRole("heading", { name: /Origin PRs/ })).toHaveText("Origin PRs 0 open of 2 PRs");
  await expect(pane.getByRole("button", { name: "#10 gh-7: Fix it" })).toHaveCSS("font-weight", "600");
  await expect(pane).toContainText(
    /issue-author opened this \d+d ago · 1 comment · assigned to some-assignee/,
  );
  await expect(pane.getByRole("list", { name: "Labels" }).getByRole("listitem")).toHaveText(["type-bug"]);
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

test("Ctrl+K waits for a chord's second key, and an unbound key or a click ends the wait", async ({
  page,
}) => {
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  const row = page.getByRole("button", { name: "#7 Issue 7" });
  const chord = page.getByText(/^(Ctrl|Cmd)\+K$/);
  await expect(row).toBeVisible();

  await page.keyboard.press("ControlOrMeta+K");
  await expect(chord).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await expect(chord).toHaveCount(0);
  await expect(row).not.toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(row).toBeFocused();

  await page.keyboard.press("ControlOrMeta+K");
  await expect(chord).toBeVisible();
  await page.getByRole("button", { name: "#8 Issue 8" }).click();
  await expect(chord).toHaveCount(0);
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

  await expect(
    page.getByRole("region", { name: "Issue view" }).getByRole("heading", { level: 2 }),
  ).toBeInViewport();
});

/** A long Issue 7 with PR 10 in its tree, in a window too short to show it whole. */
const longIssue = async (page: Page) => {
  await page.setViewportSize({ width: 1000, height: 400 });
  const paragraphs = Array.from({ length: 60 }, (_, at) => `<p>Paragraph ${String(at)}</p>`).join("");
  const tree = issue(7, false, [{ number: 10, kind: "pr", ...row, backports: [] }]);
  await mockCore(page, {
    ...answers(paragraphs),
    workbench: { ...listing(false), data: { ...listing(false).data, items: [tree, issue(8, false)] } },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: done,
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PRs of #7" }).click();
  await inList(page, "#7 Issue 7").click();
  const pane = page.getByRole("region", { name: "Issue view" });
  await expect(pane).toContainText("Paragraph 59");
  return pane;
};

/** The line named `name` in the list, as the Issue view has a button of that name too. */
const inList = (page: Page, name: string) => page.locator(".list").getByRole("button", { name });

/** Scrolls `pane` to `top` pixels from its top, and waits for the page to hear of it. */
const scrollTo = async (pane: Locator, top: number) => {
  await pane.evaluate(async (element, top) => {
    element.scrollTop = top;
    await new Promise(requestAnimationFrame);
  }, top);
};

const scrolled = (pane: Locator) => pane.evaluate((element) => element.scrollTop);

test("the Issue view comes back where the user left it after a PR from its tree", async ({ page }) => {
  const pane = await longIssue(page);
  await scrollTo(pane, 300);
  const pr = inList(page, "#10 gh-7: Fix it");

  await pr.click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(pane).toBeVisible();
  const afterEsc = await scrolled(pane);
  await scrollTo(pane, 200);
  await pr.click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(pane).toBeVisible();

  expect([afterEsc, await scrolled(pane)]).toEqual([300, 200]);
});

test("the wheel scrolls the pane under the pointer, and stops at the pane's end", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 400 });
  const paragraphs = Array.from({ length: 60 }, (_, at) => `<p>Paragraph ${String(at)}</p>`).join("");
  const many = [issue(7, false), ...Array.from({ length: 40 }, (_, at) => issue(100 + at, false))];
  await mockCore(page, {
    ...answers(paragraphs),
    workbench: { ...listing(false), data: { ...listing(false).data, items: many } },
  });
  await page.goto("/");
  await inList(page, "#7 Issue 7").click();
  const pane = page.getByRole("region", { name: "Issue view" });
  await expect(pane).toContainText("Paragraph 59");
  const list = page.locator(".list");
  const bottom = (part: Locator) => part.evaluate((element) => element.scrollHeight - element.clientHeight);

  await page.getByRole("navigation", { name: "Sidebar" }).hover();
  await page.mouse.wheel(0, 300);
  await pane.hover();
  for (let turn = 0; turn < 3; turn += 1) await page.mouse.wheel(0, 2000);
  // A scroll offset can be a fraction of a pixel.
  await expect.poll(async () => (await bottom(pane)) - (await scrolled(pane))).toBeLessThan(1);
  await list.hover();
  await page.mouse.wheel(0, 200);
  await expect.poll(() => scrolled(list)).toBeGreaterThan(0);

  // Only the wheel over the list scrolled it. Neither the window nor the screen around the panes scrolled,
  // and the window has nothing to scroll, so no scrollbar of its own at the right edge.
  expect(await scrolled(list)).toBeLessThanOrEqual(200);
  expect(await scrolled(page.locator("main"))).toBe(0);
  expect(await page.evaluate(() => document.documentElement.scrollHeight - innerHeight)).toBe(0);
});

test("an Issue's PRs fold away and come back, and Alt+← to one of them unfolds its Issue", async ({
  page,
}) => {
  await longIssue(page);
  const pr = inList(page, "#10 gh-7: Fix it");
  await pr.click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await inList(page, "#8 Issue 8").click();
  await page.getByRole("button", { name: "PRs of #7" }).click();
  await expect(pr).toHaveCount(0);

  await page.keyboard.press("Alt+ArrowLeft");
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();
  await expect(pr).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(inList(page, "#7 Issue 7")).toBeFocused();
  await expect(pr).toBeVisible();
  await page.keyboard.press("ArrowLeft");
  await expect(pr).toHaveCount(0);
  await page.keyboard.press("ArrowRight");

  await expect(pr).toBeVisible();
});

test("the Issue view starts at the top when the user opens it from the list", async ({ page }) => {
  const pane = await longIssue(page);
  await scrollTo(pane, 300);
  await inList(page, "#10 gh-7: Fix it").click();
  await expect(page.getByRole("region", { name: "PR view" })).toBeVisible();

  await inList(page, "#7 Issue 7").click();

  await expect(pane).toBeVisible();
  expect(await scrolled(pane)).toBe(0);
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

test("Ctrl+B and the palette hide the Sidebar, and the Issue list takes its room", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  const edge = page.getByRole("separator", { name: "Sidebar width" });
  const list = page.locator(".list");
  const width = async () => (await list.boundingBox())?.width ?? 0;
  await expect(sidebar).toBeVisible();
  const shown = await width();
  const palette = async (title: string) => {
    await page.keyboard.press("ControlOrMeta+Shift+P");
    await page.getByRole("combobox", { name: "Command" }).fill(title);
    await expect(page.getByRole("option")).toHaveText([title]);
    await page.keyboard.press("Enter");
  };

  await page.keyboard.press("ControlOrMeta+B");
  await expect(sidebar).toHaveCount(0);
  const hidden = await width();
  await expect(edge).toHaveCount(0);
  await palette("Show Sidebar");
  await expect(sidebar).toBeVisible();
  const back = await width();
  await palette("Hide Sidebar");
  await expect(sidebar).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.keyboard.press("ControlOrMeta+B");
  await page.keyboard.press("Escape");

  expect(hidden).toBeGreaterThan(shown + 100);
  expect(back).toBe(shown);
  await expect(sidebar).toHaveCount(0);
});

test("Ctrl+Alt+B hides the Issue list, and the Detail pane takes its room and the focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const pane = page.getByRole("region", { name: "Issue view" });
  const row = page.locator(".list").getByRole("button", { name: "#7 Issue 7" });
  const width = async () => (await pane.boundingBox())?.width ?? 0;
  await expect(pane).toBeVisible();
  const shown = await width();

  await page.keyboard.press("ControlOrMeta+Alt+B");
  await expect(row).toHaveCount(0);
  const hidden = await width();
  await expect(pane).toBeFocused();
  await page.keyboard.press("ControlOrMeta+Alt+B");

  expect(hidden).toBeGreaterThan(shown + 200);
  await expect(row).toBeFocused();
  expect(await width()).toBe(shown);
});

test("Ctrl+K Z shows only the Detail pane, and again brings back the layout", async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const pane = page.getByRole("region", { name: "Issue view" });
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  const row = page.locator(".list").getByRole("button", { name: "#7 Issue 7" });
  const width = async () => (await pane.boundingBox())?.width ?? 0;
  await expect(pane).toBeVisible();
  const shown = await width();

  await page.keyboard.press("ControlOrMeta+K");
  await page.keyboard.press("Z");
  await expect(sidebar).toHaveCount(0);
  await expect(row).toHaveCount(0);
  await expect(pane).toBeFocused();
  const alone = await width();
  await page.keyboard.press("ControlOrMeta+K");
  await page.keyboard.press("Z");

  await expect(sidebar).toBeVisible();
  await expect(row).toBeFocused();
  expect(alone).toBeGreaterThan(shown + 400);
  await expect.poll(width).toBe(shown);
});

test("Reset Layout in the palette brings back both panes, with the focus on the open item's row", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1200, height: 600 });
  await mockCore(page, answers("<p>It crashes</p>"));
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  const row = page.locator(".list").getByRole("button", { name: "#7 Issue 7" });
  const list = page.locator(".list");
  const width = async () => (await list.boundingBox())?.width ?? 0;
  await expect(page.getByRole("region", { name: "Issue view" })).toBeVisible();
  const shown = await width();
  const edge = (await page.getByRole("separator", { name: "Workbench width" }).boundingBox()) ?? {
    x: 0,
    y: 0,
  };
  await page.mouse.move(edge.x + 8, edge.y + 20);
  await page.mouse.down();
  await page.mouse.move(edge.x + 108, edge.y + 20, { steps: 4 });
  await page.mouse.up();
  const dragged = await width();
  await page.keyboard.press("ControlOrMeta+B");
  await page.keyboard.press("ControlOrMeta+Alt+B");
  await expect(row).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+Shift+P");
  await page.getByRole("combobox", { name: "Command" }).fill("Reset Layout");
  await page.keyboard.press("Enter");

  await expect(sidebar).toBeVisible();
  await expect(row).toBeFocused();
  expect(dragged).toBeGreaterThan(shown + 50);
  await expect.poll(width).toBe(shown);
});

test("the Octicons take the theme's state colours on either side", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, { ...answers("<p>It crashes</p>"), themes: { status: "ok", data: themes } });
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const pane = page.getByRole("region", { name: "Issue view" });
  const issue = pane.getByRole("img", { name: "Open issue" });
  const merged = pane.getByRole("img", { name: "Merged PR" }).first();
  const colors = async () => [
    await issue.evaluate((icon) => getComputedStyle(icon).color),
    await merged.evaluate((icon) => getComputedStyle(icon).color),
  ];
  await expect.poll(colors).toEqual(["rgb(26, 127, 55)", "rgb(130, 80, 223)"]);

  await page.emulateMedia({ colorScheme: "dark" });
  await expect.poll(colors).toEqual(["rgb(63, 185, 80)", "rgb(171, 125, 248)"]);
});
