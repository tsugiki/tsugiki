import { expect, type Page, test } from "@playwright/test";
import type {
  DiffThread_Serialize as DiffThread,
  Filter,
  ListedItem_Serialize as ListedItem,
  PrView_Serialize as PrView,
  WorkbenchRow_Serialize as WorkbenchRow,
} from "../src/lib/data/bindings.ts";
import { changeAnswer, mockCore } from "./mock-core.ts";
import { themes } from "./themes.ts";

const repository = { name: "python/cpython", conventions: "cpython", filters: [] as Filter[] } as const;

const row: WorkbenchRow = {
  number: 10,
  title: "gh-7: Fix it",
  author: "pr-author",
  state: "OPEN",
  is_draft: false,
  target_branch: "main",
  ci: "SUCCESS",
  updated_at: "2026-11-04T10:00:00Z",
  reasons: [],
  your_review: null,
};

const issue: ListedItem = {
  kind: "issue",
  number: 7,
  title: "Issue 7",
  state: "OPEN",
  people: ["issue-author"],
  updated_at: "2026-11-04T10:00:00Z",
  pull_requests: [{ ...row, kind: "pr", backports: [] }],
  attention: null,
};

const view: PrView = {
  ...row,
  content: {
    body_html: "<p>The fix</p>",
    comments: [],
    earlier_comments: 0,
    reviews: [
      {
        author: "some-reviewer",
        state: "APPROVED",
        commit: "0123abcdef",
        submitted_at: "2026-11-04T10:00:00Z",
        body_html: "",
        hidden: null,
        threads: [],
      },
    ],
    earlier_reviews: 0,
  },
};

const done = { status: "ok", data: null } as const;

test("a PR in an Issue's tree opens in the Detail pane, switches tabs, and Esc goes back", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, {
    themes: { status: "ok", data: themes },
    home: { screen: "repository", repository, others: [] as string[] },
    workbench: {
      status: "ok",
      data: { items: [issue], quiet: [], count: 0, review_requests: 0, filters: [] },
    },
    openIssue: done,
    fetchIssue: done,
    issue: { status: "ok", data: { ...issue, author: "issue-author", content: null } },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: { status: "ok", data: view },
    pullRequestFiles: {
      status: "ok",
      data: {
        state: "ready",
        head: "0123abcdef",
        base: "fedcba3210",
        files: [
          {
            path: "NEWS",
            old_path: null,
            change: { kind: "lines", added: 1, removed: 0 },
            row: 0,
            // A line this long elsewhere in the PR still leaves the halves side by side in view.
            columns: 400,
            generated: false,
            gaps: [],
            rest: null,
            pairs: [{ row: 2, removed: 0, added: 1 }],
          },
        ],
        rows: 3,
        news: [],
        threads: { kind: "shown", threads: [] },
      },
    },
    pullRequestRows: {
      status: "ok",
      data: [
        { kind: "file", path: "NEWS", old_path: null, change: { kind: "lines", added: 1, removed: 0 } },
        { kind: "hunk", text: "@@ -0,0 +1 @@" },
        { kind: "added", new: 1, text: "The fix" },
      ],
    },
    pullRequestSpans: {
      status: "ok",
      data: [
        [],
        [],
        [
          { start: 0, end: 3, highlight: "heading" },
          { start: 4, end: 7, highlight: "keyword" },
        ],
      ],
    },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByRole("button", { name: "#10 gh-7: Fix it" })).toBeFocused();
  await page.keyboard.press("Enter");

  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toContainText("The fix");
  await expect(pane).toContainText("some-reviewer approved at 0123abc");
  await page.keyboard.press("ControlOrMeta+PageDown");
  await expect(pane.getByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
  const diff = pane.getByRole("region", { name: "Diff" });
  await expect(diff).toContainText("NEWS+1 −0");
  await expect(diff).toContainText("1+The fix");
  // The theme's syntax colours, light as the system is now, and dark once it turns.
  const keyword = diff.locator(".hl-keyword");
  await expect(keyword).toHaveText("fix");
  await expect(keyword).toHaveCSS("color", "rgb(0, 0, 255)");
  await expect(diff.locator(".hl-heading")).toHaveCSS("font-weight", "600");
  await page.emulateMedia({ colorScheme: "dark" });
  await expect(keyword).toHaveCSS("color", "rgb(86, 156, 214)");
  // Side by side, the added line shows on the right half, and the key switches back.
  await page.keyboard.press("ControlOrMeta+Alt+u");
  const sideBySide = pane.getByRole("region", { name: "Side-by-side diff" });
  const added = sideBySide.locator(".side.added");
  await expect(added).toHaveText("1+The fix");
  /** Where the added side starts and ends across the diff's region, from 0 at its left to 1 at its right. */
  const across = () =>
    added.evaluate((side) => {
      const region = side.closest("[role=region]")?.getBoundingClientRect();
      const box = side.getBoundingClientRect();
      return region === undefined
        ? []
        : [(box.left - region.left) / region.width, (box.right - region.left) / region.width];
    });
  const [start = 0, end = 0] = await across();
  expect(start).toBeCloseTo(0.5, 1);
  expect(end).toBeLessThanOrEqual(1);
  // Scrolled across, the halves stay where they are, and their code moves.
  await sideBySide.evaluate((region) => {
    region.scrollLeft = 200;
  });
  // The rows move across once the scroll event has come, a frame after the scroll.
  await expect
    .poll(() =>
      sideBySide.evaluate((region) =>
        region.querySelector<HTMLElement>(".spacer")?.style.getPropertyValue("--across"),
      ),
    )
    .toBe("200px");
  await expect
    .poll(() => added.locator(".text").evaluate((text) => text.getBoundingClientRect().x))
    .toBeLessThan(await added.evaluate((side) => side.getBoundingClientRect().x));
  const [scrolledStart = 0, scrolledEnd = 0] = await across();
  expect([scrolledStart, scrolledEnd].map((at) => at.toFixed(2))).toEqual(
    [start, end].map((at) => at.toFixed(2)),
  );
  await page.keyboard.press("ControlOrMeta+Alt+u");
  await expect(diff).toContainText("1+The fix");
  await page.keyboard.press("ControlOrMeta+PageUp");
  await expect(pane).toContainText("The fix");

  await page.keyboard.press("Escape");

  await expect(page.getByRole("region", { name: "Issue view" })).toContainText("Issue 7");
  await expect(pane).toHaveCount(0);
  // The focus goes with the cursor, off the PR's row, so Enter doesn't open the PR again.
  await expect(
    page.getByRole("button", { name: "#7 Issue 7" }).and(page.locator("[aria-current=true]")),
  ).toBeFocused();
});

/** Opens a PR whose diff adds two lines to `NEWS` and has a thread at `at` with the comment `words`, and
 * whose Conversation lists the thread under `NEWS:1`. Returns the PR view's pane. */
const threaded = async (page: Page, at: DiffThread["at"], words: string) => {
  const thread = {
    id: "T1",
    path: "NEWS",
    subject: { kind: "line", side: "RIGHT", line: 1, original: 1 },
    opening: { kind: "words", words: "Say what it fixes" },
    replies: 0,
    resolution: "open",
    anchor: "current",
  } as const;
  const reviewed: PrView = {
    ...view,
    content: view.content && {
      ...view.content,
      reviews: view.content.reviews.map((review) => ({ ...review, threads: [thread] })),
    },
  };
  await mockCore(page, {
    home: { screen: "repository", repository, others: [] as string[] },
    workbench: {
      status: "ok",
      data: { items: [issue], quiet: [], count: 0, review_requests: 0, filters: [] },
    },
    openIssue: done,
    fetchIssue: done,
    issue: { status: "ok", data: { ...issue, author: "issue-author", content: null } },
    openPullRequest: done,
    fetchPullRequest: done,
    pullRequest: { status: "ok", data: reviewed },
    pullRequestFiles: {
      status: "ok",
      data: {
        state: "ready",
        head: "0123abcdef",
        base: "fedcba3210",
        files: [
          {
            path: "NEWS",
            old_path: null,
            change: { kind: "lines", added: 2, removed: 0 },
            row: 0,
            columns: 9,
            generated: false,
            gaps: [],
            rest: null,
            pairs: [{ row: 2, removed: 0, added: 2 }],
          },
        ],
        rows: 4,
        news: [],
        threads: {
          kind: "shown",
          threads: [
            {
              id: "T1",
              at,
              resolution: "open",
              comments: [
                {
                  author: "some-reviewer",
                  created_at: "2026-11-04T10:00:00Z",
                  body_html: `<p>${words}</p>`,
                  hidden: null,
                  submission: "submitted",
                },
              ],
              later_comments: 0,
            },
          ],
        },
      },
    },
    pullRequestRows: {
      status: "ok",
      data: [
        { kind: "file", path: "NEWS", old_path: null, change: { kind: "lines", added: 2, removed: 0 } },
        { kind: "hunk", text: "@@ -0,0 +1,2 @@" },
        { kind: "added", new: 1, text: "The fix" },
        { kind: "added", new: 2, text: "And more" },
      ],
    },
    pullRequestSpans: { status: "ok", data: [[], [], [], []] },
  });
  await page.goto("/");
  await page.getByRole("button", { name: "#10 gh-7: Fix it" }).click();
  return page.getByRole("region", { name: "PR view" });
};

test("a thread's place in the Conversation opens it in Files, under its line in either view", async ({
  page,
}) => {
  const pane = await threaded(page, { kind: "row", row: 2 }, "Say what it fixes. ".repeat(40));

  await pane.getByRole("button", { name: "NEWS:1" }).click();

  await expect(pane.getByRole("tab", { name: "Files" })).toHaveAttribute("aria-selected", "true");
  const box = pane.getByRole("group", { name: "Review thread" });
  await expect(box).toContainText("Say what it fixes.");
  /** The gaps in pixels from line `line` to the thread under it, and from the thread to line `next`. */
  const gaps = (line: string, next: string) =>
    box.evaluate(
      (thread, [line, next]) => {
        const rows = [...document.querySelectorAll(".row")];
        const top = (text: string) =>
          rows.find((row) => row.textContent.includes(text))?.getBoundingClientRect();
        const own = thread.getBoundingClientRect();
        return [own.top - (top(line)?.bottom ?? 0), (top(next)?.top ?? 0) - own.bottom];
      },
      [line, next] as const,
    );
  // The thread wraps to several rows' height, and the next line clears it by less than a row.
  await expect.poll(() => gaps("The fix", "And more")).toEqual([0, expect.any(Number)]);
  const [, below = -1] = await gaps("The fix", "And more");
  expect(below).toBeGreaterThanOrEqual(0);
  expect(below).toBeLessThan(20);
  expect(await box.evaluate((thread) => thread.getBoundingClientRect().height)).toBeGreaterThan(40);

  await page.keyboard.press("ControlOrMeta+Alt+u");
  await expect(pane.getByRole("region", { name: "Side-by-side diff" })).toBeVisible();
  await expect.poll(() => gaps("The fix", "And more")).toEqual([0, expect.any(Number)]);
});

test("an Outdated thread sits at the top of its file with the last lines of its code, all of them on Show more", async ({
  page,
}) => {
  const rows = (count: number) =>
    Array.from({ length: count }, (_, at) => ({
      kind: "added" as const,
      new: at + 1,
      text: `Line ${String(at + 1)}`,
    }));
  const code = { rows: [{ kind: "hunk" as const, text: "@@ -0,0 +1,6 @@" }, ...rows(6)], spans: [] };
  const pane = await threaded(page, { kind: "outdated", file: 0, code }, "Say what it fixes.");

  await pane.getByRole("button", { name: "NEWS:1" }).click();

  const box = pane.getByRole("group", { name: "Review thread" });
  await expect(box.locator(".code .row")).toHaveText(["3+Line 3", "4+Line 4", "5+Line 5", "6+Line 6"]);
  /** The pixels from the top of the file's header to the thread, a row with the header's border overhanging
   * it, and from the thread to the hunk's row. */
  const gaps = () =>
    box.evaluate((thread) => {
      const [header, hunk] = [".row.file", ".row.hunk"].map((row) =>
        document.querySelector(row)?.getBoundingClientRect(),
      );
      const own = thread.getBoundingClientRect();
      return [own.top - (header?.top ?? 0), (hunk?.top ?? 0) - own.bottom];
    });
  await expect.poll(gaps).toEqual([20, expect.any(Number)]);
  await box.getByRole("button", { name: "Show more" }).click();
  await expect(box.locator(".code .row")).toHaveCount(7);
  // The hunk's row moves down to clear the whole code, by less than a row.
  await expect.poll(async () => (await gaps())[1]).toBeGreaterThanOrEqual(0);
  expect((await gaps())[1]).toBeLessThan(20);
});

test("Ctrl+F searches the diff, marks the match it moves to, and Escape gives the diff its focus back", async ({
  page,
}) => {
  const pane = await threaded(page, { kind: "row", row: 2 }, "Say what it fixes.");
  await changeAnswer(page, "pullRequestFind", {
    status: "ok",
    data: { matches: [{ row: 3, start: 4, end: 8 }], more: false },
  });
  await pane.getByRole("tab", { name: "Files" }).click();
  await expect(pane.getByRole("region", { name: "Diff" })).toContainText("And more");

  await page.keyboard.press("ControlOrMeta+f");
  await page.keyboard.type("more");

  const search = pane.getByRole("search", { name: "Find in the diff" });
  await expect(search).toContainText("1 of 1");
  await expect(pane.locator(".row .current")).toHaveText("more");
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);
  await expect(pane.getByRole("region", { name: "Diff" })).toBeFocused();
});
