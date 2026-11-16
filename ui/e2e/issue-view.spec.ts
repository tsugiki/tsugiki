import { expect, type Locator, type Page, test } from "@playwright/test";
import type {
  Filter,
  IssueView_Serialize as IssueView,
  ListedItem_Serialize as ListedItem,
  TreeEntry_Serialize as TreeEntry,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, emitFromCore, mockCore } from "./mock-core.ts";
import { cspViolations, imageHosts, serveUnderTauriCsp, watchCspViolations } from "./tauri-csp.ts";
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
  your_review: { kind: "reviewed" },
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
    count: waiting ? 1 : 0,
    review_requests: 0,
    stale_reviews: 0,
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
  await expect(pane.getByRole("heading", { name: /Origin PRs/ })).toHaveText("Origin PRs");
  await expect(pane.getByRole("button", { name: "#10 gh-7: Fix it" })).toHaveCSS("font-weight", "600");
  await expect(pane).toContainText(
    /issue-author opened this \d+d ago · 1 comment · assigned to some-assignee/,
  );
  await expect(pane.getByRole("list", { name: "Labels" }).getByRole("listitem")).toHaveText(["type-bug"]);
  await expect(pane).toContainText("It crashes");
  await expect(pane).toContainText("Which build?");
  await expect(pane).toContainText("Merged · CI passed · Reviewed");
  await expect(pane).toContainText("3.14 Merged");

  await changeAnswer(page, "workbench", listing(false));
  await emitFromCore(page, "changed", { repository: "python/cpython", items: [7] });
  await expect(page.getByRole("button", { name: "#7 Issue 7" }).locator("strong")).toHaveCount(0);
});

test("a row above the tree counts the PRs of each state, and mutes a state at 0", async ({ page }) => {
  // An open PR with a draft Backport, and a merged PR with its Backport folded away: no PR is closed.
  const counted: IssueView = {
    ...view("<p>It crashes</p>"),
    pull_requests: [
      {
        kind: "pr",
        number: 10,
        ...row,
        state: "OPEN",
        backports: [
          { number: 11, ...row, state: "OPEN", is_draft: true, target_branch: "3.14", backport: "manual" },
        ],
      },
    ],
    merged: [
      {
        kind: "pr",
        number: 20,
        ...row,
        backports: [{ number: 21, ...row, target_branch: "3.14", backport: "bot" }],
      },
    ],
    merged_count: 2,
  };
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, {
    ...answers(""),
    issue: { status: "ok", data: counted },
    themes: { status: "ok", data: themes },
  });
  await page.goto("/");

  await page.getByRole("button", { name: "#7 Issue 7" }).click();

  const pane = page.getByRole("region", { name: "Issue view" });
  const entries = pane.getByRole("list", { name: "PR counts" }).getByRole("listitem");
  await expect(entries).toHaveText([
    "Open, 1",
    "Draft, 1",
    "Merged, 2",
    "Closed, 0",
    "Bold: Needs attention, 1",
  ]);
  // The comma is for a screen reader alone: on screen the count follows the word after a gap.
  await expect(entries.first().getByText(",", { exact: true })).toHaveCSS("position", "absolute");
  await expect(pane.getByText("Needs attention", { exact: false })).toHaveCount(1);

  // Light Modern's text, muted text, and green for an open PR. Its muted state colour is its muted text's.
  const [fg, muted, green] = ["rgb(59, 59, 59)", "rgb(118, 118, 118)", "rgb(26, 127, 55)"];
  await expect(entries.nth(0).getByText("Open", { exact: true })).toHaveCSS("color", fg);
  await expect(entries.nth(0).getByText("1", { exact: true })).toHaveCSS("color", muted);
  await expect(entries.nth(0).getByTitle("Open PR")).toHaveCSS("color", green);
  // The Closed icon fades from its red, and its word with it.
  await expect(entries.nth(3).getByText("Closed", { exact: true })).toHaveCSS("color", muted);
  await expect(entries.nth(3).getByTitle("Closed PR")).toHaveCSS("color", muted);
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
        count: 0,
        review_requests: 0,
        stale_reviews: 0,
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
  await serveUnderTauriCsp(page);
  await mockCore(
    page,
    answers(
      '<img alt="handler" src="data:," onerror="window.ranFromDescription = true">' +
        '<img alt="remote" src="https://example.com/favicon.ico">',
    ),
  );
  const asked: string[] = [];
  page.on("request", (request) => {
    if (request.url().startsWith("https://")) asked.push(request.url());
  });
  await page.goto("/");

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const remote = page.getByAltText("remote");
  await expect(remote).toBeAttached();
  await expect(page.getByAltText("handler")).toBeAttached();
  // Each image has had its chance to fail by the time the page settles.
  await page.waitForLoadState("networkidle");

  expect(await page.evaluate(() => "ranFromDescription" in window)).toBe(false);
  expect(asked).toEqual([]);
});

/** GitHub's image proxy, which its rendered HTML sends each third-party image through (#761). */
const camo = "https://camo.githubusercontent.com";
const codspeedDark = `${camo}/1790/636f6473706565642d6461726b`;
const codspeedLight = `${camo}/c88a/636f6473706565642d6c69676874`;
/** CodSpeed's button as GitHub renders it: a source for each side, each through camo. */
const codspeedButton =
  "<picture>" +
  `<source media="(prefers-color-scheme: dark)" srcset="${codspeedDark}" ` +
  'data-canonical-src="https://codspeed.io/pr-report/open-in-codspeed-dark.svg">' +
  `<source media="(prefers-color-scheme: light)" srcset="${codspeedLight}" ` +
  'data-canonical-src="https://codspeed.io/pr-report/open-in-codspeed-light.svg">' +
  `<img alt="Open in CodSpeed" src="${codspeedLight}" width="169" height="32" ` +
  'data-canonical-src="https://codspeed.io/pr-report/open-in-codspeed-light.svg">' +
  "</picture>";

/** Serves the page under Tauri's CSP, keeping what it refuses, and answers each image the policy's hosts
 * are asked for with a small picture. */
const serveImagesUnderCsp = async (page: Page) => {
  await watchCspViolations(page);
  await serveUnderTauriCsp(page);
  await page.route(
    (address) => imageHosts.includes(address.origin),
    (route) =>
      route.fulfill({
        contentType: "image/svg+xml",
        body: '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"></svg>',
      }),
  );
};

/** The addresses the content security policy refused, each with the directive that refused it. */
const refused = async (page: Page) =>
  (await cspViolations(page)).map(({ directive, blocked }) => `${directive} ${blocked}`);

/** Whether the image `alt` names has loaded. */
const loaded = (page: Page, alt: string) =>
  page.getByAltText(alt).evaluate((image: HTMLImageElement) => image.complete && image.naturalWidth > 0);

test("a description shows the images GitHub's image hosts serve, and loads no other host's", async ({
  page,
}) => {
  const images = {
    "Dependabot compatibility score": `${camo}/f3d2/6465706`,
    ":shipit:": "https://github.githubassets.com/images/icons/emoji/shipit.png",
    "an older upload": "https://user-images.githubusercontent.com/1/2-a1b2.png",
    "an upload": "https://private-user-images.githubusercontent.com/1/3-c3d4.png?jwt=x",
    "a workflow's badge": "https://github.com/octo-org/octo-repo/actions/workflows/ci.yml/badge.svg",
    "a file of a Repository": "https://raw.githubusercontent.com/octo-org/octo-repo/main/docs/screen.png",
    "a Git LFS file": "https://media.githubusercontent.com/media/octo-org/octo-repo/0a1b/sprite.webp",
    "an avatar": "https://avatars.githubusercontent.com/u/1?v=4",
  };
  await serveImagesUnderCsp(page);
  await mockCore(
    page,
    answers(
      Object.entries(images)
        .map(([alt, address]) => `<p><img alt="${alt}" src="${address}"></p>`)
        .join("") +
        '<p><img alt="a stranger" src="https://codspeed.io/pr-report/open-in-codspeed-light.svg"></p>',
    ),
  );
  await page.goto("/");

  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  for (const alt of Object.keys(images)) {
    await expect.poll(() => loaded(page, alt), alt).toBe(true);
  }
  await expect
    .poll(() => refused(page))
    .toEqual(["img-src https://codspeed.io/pr-report/open-in-codspeed-light.svg"]);
  expect(await loaded(page, "a stranger")).toBe(false);
});

test("CodSpeed's button shows the variant of the theme's side", async ({ page }) => {
  await serveImagesUnderCsp(page);
  await mockCore(page, answers(codspeedButton));
  // The window turns to the side of the theme in force, and the webview's media query with it.
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  const button = page.getByAltText("Open in CodSpeed");

  await expect.poll(() => loaded(page, "Open in CodSpeed")).toBe(true);
  await expect.poll(() => button.evaluate((image: HTMLImageElement) => image.currentSrc)).toBe(codspeedDark);

  await page.emulateMedia({ colorScheme: "light" });
  await expect.poll(() => button.evaluate((image: HTMLImageElement) => image.currentSrc)).toBe(codspeedLight);
  await expect.poll(() => loaded(page, "Open in CodSpeed")).toBe(true);
  expect(await refused(page)).toEqual([]);
});

test("a refresh that signs an upload's address afresh leaves the description as it was", async ({ page }) => {
  const upload = (signature: string) =>
    `https://private-user-images.githubusercontent.com/1/2-a1b2.png?jwt=${signature}`;
  const screenshot = (signature: string) =>
    `<details><summary>Steps</summary><a href="${upload(signature)}">` +
    `<img alt="a screenshot" src="${upload(signature)}"></a></details>`;
  await serveImagesUnderCsp(page);
  const fetched: string[] = [];
  page.on("request", (request) => {
    if (request.url().startsWith("https://private-user-images.")) fetched.push(request.url());
  });
  await mockCore(page, answers(screenshot("one")));
  await page.goto("/");
  await page.getByRole("button", { name: "#7 Issue 7" }).click();
  await page.getByText("Steps").click();
  const image = page.getByAltText("a screenshot");
  await expect.poll(() => loaded(page, "a screenshot")).toBe(true);
  await image.evaluate((drawn) => Object.assign(drawn, { drawnBefore: true }));

  await changeAnswer(page, "issue", held(screenshot("two")));
  // Listening from before the refresh, which would ask for the fresh address at once if it loaded it.
  const refetch = page.waitForRequest(upload("two"), { timeout: 1000 }).then(
    () => true,
    () => false,
  );
  await emitFromCore(page, "changed", { repository: "python/cpython", items: [7] });

  await expect(page.getByRole("link", { name: "a screenshot" })).toHaveAttribute("href", upload("two"));
  expect(await image.evaluate((shown) => "drawnBefore" in shown)).toBe(true);
  await expect(page.locator(".rendered details")).toHaveAttribute("open", "");
  expect(await refetch).toBe(false);
  expect(fetched).toEqual([upload("one")]);
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
    // The box opens with `>` typed, which marks the commands, and `fill` replaces it.
    await page.getByRole("combobox", { name: "Command" }).fill(`>${title}`);
    // One row, named for what it does: its key describes it, and stays out of its name.
    await expect(page.getByRole("option")).toHaveAccessibleName(title);
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
  await page.getByRole("combobox", { name: "Command" }).fill(">Reset Layout");
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
