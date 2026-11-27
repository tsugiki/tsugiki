import { expect, type Locator, type Page, test } from "@playwright/test";
import type {
  DiffThread_Serialize as DiffThread,
  PrView_Serialize as PrView,
} from "../src/lib/data/bindings.ts";
import {
  diffFile,
  diffThread,
  done,
  fileRow,
  folderRow,
  loadedSettings,
  ok,
  readyFiles,
  reviewComment,
  workbenchPage,
} from "../src/lib/testing/fixtures.ts";
import {
  dragEdge,
  expectShortTitleHugsItsText,
  expectTitleBesideItsIcon,
  home,
  issue,
  issueView,
  own,
  prView,
} from "./fixtures.ts";
import { changeAnswer, mockCore } from "./mock-core.ts";
import { themes } from "./themes.ts";

/** PR #10, approved at the head its diff reads from. */
const view = prView([
  {
    id: 1,
    author: "some-reviewer",
    state: "APPROVED",
    commit: "0123abcdef",
    submitted_at: "2026-11-04T10:00:00Z",
    body_html: "",
    hidden: null,
    threads: [],
  },
]);

/** The commits PR #10's diff goes between. */
const heads = { head: "0123abcdef", base: "fedcba3210" };

test("a PR in an Issue's tree opens in the Detail pane, switches tabs, and Esc goes back", async ({
  page,
}) => {
  await page.emulateMedia({ colorScheme: "light" });
  await mockCore(page, {
    themes: ok(themes),
    home,
    workbench: workbenchPage([issue]),
    openIssue: done,
    issue: ok(issueView),
    openPullRequest: done,
    pullRequest: ok(view),
    pullRequestFiles: ok(
      readyFiles(3, {
        ...heads,
        files: [
          diffFile("NEWS", 0, {
            // A line this long elsewhere in the PR still leaves the halves side by side in view.
            columns: 400,
            pairs: [{ row: 2, removed: 0, added: 1 }],
          }),
        ],
      }),
    ),
    pullRequestRows: ok([
      { kind: "file", path: "NEWS", old_path: null, change: { kind: "lines", added: 1, removed: 0 } },
      { kind: "hunk", text: "@@ -0,0 +1 @@" },
      { kind: "added", new: 1, text: "The fix" },
    ]),
    pullRequestSpans: ok([
      [],
      [],
      [
        { start: 0, end: 3, highlight: "heading" },
        { start: 4, end: 7, highlight: "keyword" },
      ],
    ]),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();

  // The Issue starts folded, and → unfolds it.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
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

test("a title that wraps keeps the state icon on its first line", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  const title =
    "A title long enough that it cannot fit on one line beside its icon in a narrow pane, whatever the Theme";
  await mockCore(page, {
    home,
    workbench: workbenchPage([{ ...own, title }]),
    openPullRequest: done,
    pullRequest: ok({
      ...view,
      issues: [],
      title,
      holding: { key: { kind: "pull_request", number: own.number }, mark: view.holding.mark },
    }),
  });
  await page.goto("/");

  await page.getByRole("button", { name: `#157178 ${title}` }).click();

  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toBeVisible();
  await expectTitleBesideItsIcon(pane.getByRole("heading", { level: 2 }));
});

test("a title that fits on one line leaves the rest of its heading to no link", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  const title = "Short fix";
  await mockCore(page, {
    home,
    workbench: workbenchPage([{ ...own, title }]),
    openPullRequest: done,
    pullRequest: ok({
      ...view,
      number: own.number,
      issues: [],
      title,
      holding: { key: { kind: "pull_request", number: own.number }, mark: view.holding.mark },
    }),
    openOnGithub: done,
  });
  await page.goto("/");

  await page.getByRole("button", { name: `#157178 ${title}` }).click();

  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toBeVisible();
  await expectShortTitleHugsItsText(
    page,
    pane.getByRole("heading", { level: 2 }),
    pane.getByRole("button", { name: `#157178 ${title}` }),
  );
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
    themes: ok(themes),
    home,
    workbench: workbenchPage([issue]),
    openIssue: done,
    issue: ok(issueView),
    openPullRequest: done,
    pullRequest: ok(reviewed),
    pullRequestFiles: ok(
      readyFiles(4, {
        ...heads,
        files: [
          diffFile("NEWS", 0, {
            change: { kind: "lines", added: 2, removed: 0 },
            columns: 9,
            pairs: [{ row: 2, removed: 0, added: 2 }],
          }),
        ],
        threads: {
          kind: "shown",
          threads: [
            { ...diffThread("T1", at), comments: [reviewComment({ id: 2, body_html: `<p>${words}</p>` })] },
          ],
        },
      }),
    ),
    pullRequestRows: ok([
      { kind: "file", path: "NEWS", old_path: null, change: { kind: "lines", added: 2, removed: 0 } },
      { kind: "hunk", text: "@@ -0,0 +1,2 @@" },
      { kind: "added", new: 1, text: "The fix" },
      { kind: "added", new: 2, text: "And more" },
    ]),
    pullRequestSpans: ok([[], [], [], []]),
  });
  await page.goto("/");
  await page.getByRole("button", { name: "PRs of #7" }).click();
  await page.getByRole("button", { name: "#10 gh-7: Fix it" }).click();
  return page.getByRole("region", { name: "PR view" });
};

test("a thread's place in the Conversation opens it in Files, under its line in either view", async ({
  page,
}) => {
  const pane = await threaded(page, { kind: "row", row: 2, side: "RIGHT" }, "Say what it fixes. ".repeat(40));

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

test("a click anywhere on a thread's line folds it, but a click on its author opens the person Filter", async ({
  page,
}) => {
  const pane = await threaded(page, { kind: "row", row: 2, side: "RIGHT" }, "Say what it fixes.");
  await pane.getByRole("button", { name: "NEWS:1" }).click();

  const box = pane.getByRole("group", { name: "Review thread" });
  const head = box.locator(".head");
  const toggle = head.getByRole("button").first();
  // A point on the line past the chevron and the author's name, which the fold's cover still carries.
  const away = { position: { x: 150, y: 10 } };

  await head.click(away);
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(box).not.toContainText("Say what it fixes.");

  await head.click(away);
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(box).toContainText("Say what it fixes.");

  await head.getByRole("button", { name: "some-reviewer", exact: true }).click();
  const sidebar = page.getByRole("navigation", { name: "Sidebar" });
  await expect(sidebar.getByRole("button", { name: "some-reviewer unsaved" })).toHaveAttribute(
    "aria-current",
    "true",
  );
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

test("the Files button, Ctrl+K F and × open and close the rail, its left edge sizes it, and Esc in it goes back to the diff", async ({
  page,
}) => {
  // Wide enough for the Detail pane to keep 240 px of the diff beside the widened rail.
  await page.setViewportSize({ width: 1800, height: 700 });
  const pane = await threaded(page, { kind: "row", row: 2, side: "RIGHT" }, "Say what it fixes.");
  const rail = pane.getByRole("complementary", { name: "Rail" });
  const button = pane.getByRole("button", { name: "Files in the rail" });
  const width = async () => (await rail.boundingBox())?.width ?? 0;

  await button.click();
  await expect(rail).toBeVisible();
  await expect(button).toHaveAttribute("aria-pressed", "true");
  const opened = await width();
  // A drag to the left widens the rail.
  await dragEdge(pane.getByRole("separator", { name: "Rail width" }), -100);
  const dragged = await width();
  // It stays open, at its width, on the Files tab, the PR's and not the rail's.
  await pane.locator("#pr-tab-files").click();
  const diff = pane.getByRole("region", { name: "Diff" });
  await expect(diff).toContainText("And more");
  const onFiles = await width();
  await rail.getByRole("button", { name: "Close the rail" }).focus();
  await page.keyboard.press("Escape");
  await expect(diff).toBeFocused();
  await expect(rail).toBeVisible();
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("f");
  await expect(rail).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("f");
  await expect(rail).toBeVisible();
  // WebKit gives a clicked button no focus, so the keys press × here, which the rail then hands on.
  await rail.getByRole("button", { name: "Close the rail" }).focus();
  await page.keyboard.press("Enter");

  await expect(rail).toHaveCount(0);
  await expect(button).toHaveAttribute("aria-pressed", "false");
  await expect(diff).toBeFocused();
  expect([opened, dragged, onFiles]).toEqual([280, 380, 380]);
});

test("Ctrl+F searches the diff, marks the match it moves to, and Escape gives the diff its focus back", async ({
  page,
}) => {
  const pane = await threaded(page, { kind: "row", row: 2, side: "RIGHT" }, "Say what it fixes.");
  await changeAnswer(page, "pullRequestFind", ok({ matches: [{ row: 3, start: 4, end: 8 }], more: false }));
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

test("Ctrl+Shift+F searches the PR's code from the rail, and a match opens at its line in the diff or the file view", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 700 });
  const pane = await threaded(page, { kind: "row", row: 2, side: "RIGHT" }, "Say what it fixes.");
  const line = (number: number, text: string, start: number, end: number) => ({
    line: number,
    text,
    cut: "neither" as const,
    ranges: [{ start, end }],
  });
  await changeAnswer(page, "textSearch", ok({ kind: "running", id: 1 }));
  await changeAnswer(page, "textSearchState", {
    state: "results",
    id: 1,
    files: [
      { path: "Lib/os.py", generated: false, lines: [line(12, "# more to come", 2, 6)] },
      { path: "NEWS", generated: false, lines: [line(2, "And more", 4, 8)] },
    ],
    matches: 2,
    end: "done",
  });
  await changeAnswer(page, "pullRequestRow", ok({ kind: "row", row: 3 }));
  await changeAnswer(
    page,
    "definitionCode",
    ok({ count: 12, lines: Array.from({ length: 12 }, (_, at) => (at === 11 ? "# more to come" : "")) }),
  );
  await changeAnswer(page, "definitionSpans", ok(Array.from({ length: 12 }, () => [])));

  await page.keyboard.press("ControlOrMeta+Shift+f");
  const rail = pane.getByRole("complementary", { name: "Rail" });
  const box = rail.getByRole("textbox", { name: "Search the PR's code" });
  await expect(box).toBeFocused();
  await page.keyboard.type("more");
  await expect(rail).toContainText("2 results in 2 files");
  const results = rail.getByRole("tree", { name: "Search results" });
  await expect(results.getByRole("treeitem")).toHaveCount(4);
  await expect(results.locator("mark").first()).toHaveText("more");
  // ↓ goes to the list, and Enter on NEWS's line lands on it in the diff, which takes the focus.
  await page.keyboard.press("ArrowDown");
  await expect(results).toBeFocused();
  await page.keyboard.press("End");
  // The mark on the line it lands on fades and goes. Held, it stays while the test reads its colour, on a
  // slow runner too.
  await page.evaluate(() => {
    const hold = new CSSStyleSheet();
    hold.replaceSync(".mark { animation-play-state: paused !important; }");
    document.adoptedStyleSheets = [...document.adoptedStyleSheets, hold];
  });
  await page.keyboard.press("Enter");
  const diff = pane.getByRole("region", { name: "Diff" });
  const landed = await diff.locator(".mark").evaluate((mark) => getComputedStyle(mark).backgroundColor);
  await expect(diff).toBeFocused();
  await expect(pane.locator("#pr-tab-files")).toHaveAttribute("aria-selected", "true");
  await expect(rail).toBeVisible();
  // A click on the match in a file the PR doesn't change opens the file view at its line.
  await results.getByRole("treeitem").nth(1).click();

  const view = pane.getByRole("region", { name: "File view" });
  await expect(view).toContainText("# more to come");
  await expect(pane.getByRole("button", { name: "← Back to PR #10's diff" })).toBeVisible();
  // The file view draws its code at the diff's size, and marks the line it landed on as the diff does
  // (#754).
  await expect(diff).toHaveCSS("font-size", "12px");
  await expect(view.getByRole("region", { name: "Definition's code" })).toHaveCSS("font-size", "12px");
  expect(landed).not.toBe("rgba(0, 0, 0, 0)");
  await expect(view.locator(".line.landed")).toHaveText("12# more to come");
  await expect(view.locator(".line.landed")).toHaveCSS("background-color", landed);
});

/** A new file at `path` whose header is row `row`, adding `added` lines, which together are taller than the
 * window. */
const newFile = (path: string, row: number, added: number) =>
  diffFile(path, row, {
    change: { kind: "lines", added, removed: 0 },
    columns: 20,
    pairs: [{ row: row + 2, removed: 0, added }],
  });

/** The rows of that file, its first line saying `first`. */
const fileRows = (path: string, added: number, first: string) => [
  { kind: "file" as const, path, old_path: null, change: { kind: "lines" as const, added, removed: 0 } },
  { kind: "hunk" as const, text: `@@ -0,0 +1,${String(added)} @@` },
  ...Array.from({ length: added }, (_, at) => ({
    kind: "added" as const,
    new: at + 1,
    text: at === 0 ? first : `line ${String(at + 1)}`,
  })),
];

/** The distance in whole pixels between the top of the `diff` region and the header of the file at `path`:
 * the NEWS entries above the rows can end part of the way into a pixel. */
const headerBelowTop = (diff: Locator, path: string) =>
  diff.evaluate((region, path) => {
    const header = [...region.querySelectorAll(".row.file")].find((row) => row.textContent.includes(path));
    return header === undefined
      ? undefined
      : Math.trunc(Math.abs(header.getBoundingClientRect().top - region.getBoundingClientRect().top));
  }, path);

test("a PR opens one file at a time as the settings say, walks its files, and switches to all of them in place", async ({
  page,
}) => {
  const news = "Misc/NEWS.d/next/Library/2026-11-12.gh-issue-7.abc.rst";
  await mockCore(page, {
    settings: loadedSettings({ diff: { files: "one", peek: "inline" } }),
    home,
    workbench: workbenchPage([issue]),
    openPullRequest: done,
    pullRequest: ok(view),
    pullRequestFiles: ok(
      readyFiles(142, {
        ...heads,
        files: [newFile("Lib/ast.py", 0, 98), newFile(news, 100, 40)],
        news: [
          {
            path: news,
            section: "Library",
            issue: null,
            blocks: [{ kind: "paragraph", pieces: [{ kind: "text", text: "Fixed the parser." }] }],
          },
        ],
      }),
    ),
    pullRequestRows: ok([
      ...fileRows("Lib/ast.py", 98, "import ast"),
      ...fileRows(news, 40, "Fixed the parser."),
    ]),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toContainText("The fix");
  await page.keyboard.press("ControlOrMeta+PageDown");

  // The NEWS entries come first, on a page of their own.
  const bar = pane.getByRole("navigation", { name: "File on show" });
  await expect(bar).toContainText("NEWS entries");
  await expect(pane.getByRole("article", { name: "NEWS entry in Library" })).toContainText(
    "Fixed the parser.",
  );
  const diff = pane.getByRole("region", { name: "Diff" });
  await page.keyboard.press("Alt+PageDown");
  await expect(bar).toContainText("1 of 2");
  await expect(bar).toContainText("Lib/ast.py");
  await expect(diff).toContainText("import ast");
  await expect(diff).not.toContainText("Fixed the parser.");
  await page.keyboard.press("Alt+PageDown");
  await expect(bar).toHaveText(`‹2 of 2› ${news}`);
  await page.keyboard.press("Alt+PageDown");
  await expect(bar).toContainText("2 of 2");
  await page.keyboard.press("Alt+PageUp");
  await expect(bar).toContainText("1 of 2");
  // F7 carries on into the next file at the end of one.
  await page.keyboard.press("F7");
  await page.keyboard.press("F7");
  await expect(bar).toContainText("2 of 2");
  await expect(diff).toContainText("Fixed the parser.");

  const belowTop = (path: string) => headerBelowTop(diff, path);
  // All the files, with the file that showed at the top.
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("o");
  const modes = pane.getByRole("group", { name: "Files" });
  const all = modes.getByRole("button", { name: "All files" });
  const one = modes.getByRole("button", { name: "One file" });
  await expect(all).toHaveAttribute("aria-pressed", "true");
  await expect(one).toHaveAttribute("aria-pressed", "false");
  await expect(bar).toHaveCount(0);
  await expect.poll(() => belowTop(news)).toBe(0);
  // Alt+PageUp goes to the header of the file before.
  await page.keyboard.press("Alt+PageUp");
  await expect.poll(() => belowTop("Lib/ast.py")).toBe(0);
  // The first file stays at the top as the NEWS entries above it go, and as they come back.
  await one.click();
  await expect(bar).toContainText("1 of 2");
  await expect(one).toHaveAttribute("aria-pressed", "true");
  await expect.poll(() => belowTop("Lib/ast.py")).toBe(0);
  await all.click();
  await expect(bar).toHaveCount(0);
  await expect.poll(() => belowTop("Lib/ast.py")).toBe(0);
});

test("a stale PR says what changed since the Review over both tabs, and Review changes scrolls to the first changed file", async ({
  page,
}) => {
  const paths = ["Doc/whatsnew.rst", "Lib/ast.py", "Lib/os.py", "Lib/re.py"];
  const sizes = [40, 98, 40, 40];
  const starts = sizes.map((_, at) => sizes.slice(0, at).reduce((sum, size) => sum + size + 2, 0));
  const stale: PrView = {
    ...view,
    your_review: {
      kind: "stale_review",
      reviewed: "abc1234def5678abc1234def5678abc1234def56",
      head: "0123abcdef",
      files: [
        { path: "Lib/ast.py", whitespace_only: false },
        { path: "Lib/os.py", whitespace_only: true },
      ],
    },
  };
  await mockCore(page, {
    themes: ok(themes),
    home,
    workbench: workbenchPage([issue]),
    openPullRequest: done,
    pullRequest: ok(stale),
    pullRequestFiles: ok(
      readyFiles(
        sizes.reduce((sum, size) => sum + size + 2, 0),
        { ...heads, files: paths.map((path, at) => newFile(path, starts[at] ?? 0, sizes[at] ?? 0)) },
      ),
    ),
    pullRequestRows: ok(paths.flatMap((path, at) => fileRows(path, sizes[at] ?? 0, `first of ${path}`))),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toContainText("The fix");

  const line = pane.locator(".stale");
  await expect(line).toHaveText(
    "Your Review at abc1234 is stale: 2 files changed, 1 only in whitespace Review changes",
  );
  await page.keyboard.press("ControlOrMeta+PageDown");
  const diff = pane.getByRole("region", { name: "Diff" });
  await expect(line).toContainText("Your Review at abc1234 is stale");
  // The file the Review left unchanged starts folded.
  await expect(diff.locator(".row.file").first()).toHaveText("▸Doc/whatsnew.rst+40 −0");
  await expect(diff.locator(".row.file").nth(1)).toHaveText("▾Lib/ast.py+98 −0Changed since your Review");
  // Away from the first changed file, Review changes brings its header to the top.
  await page.keyboard.press("Alt+PageDown");
  await page.keyboard.press("Alt+PageDown");
  await expect.poll(() => headerBelowTop(diff, "Lib/os.py")).toBe(0);
  await expect(diff).toContainText("Whitespace only");
  await line.getByRole("button", { name: "Review changes" }).click();
  await expect.poll(() => headerBelowTop(diff, "Lib/ast.py")).toBe(0);

  // Ctrl+K J opens the files the Review left unchanged.
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("j");
  await page.keyboard.press("Alt+PageUp");
  await expect(diff).toContainText("first of Doc/whatsnew.rst");
});

test("the rail's tree jumps to a file from either tab, marks the file on screen as the diff scrolls, and filters", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1400, height: 700 });
  const paths = ["Doc/whatsnew.rst", "Lib/ast.py", "Lib/os.py", "setup.py"];
  const sizes = [40, 98, 40, 3];
  const starts = sizes.map((_, at) => sizes.slice(0, at).reduce((sum, size) => sum + size + 2, 0));
  await mockCore(page, {
    themes: ok(themes),
    home,
    workbench: workbenchPage([issue]),
    openPullRequest: done,
    pullRequest: ok(view),
    pullRequestFiles: ok(
      readyFiles(
        sizes.reduce((sum, size) => sum + size + 2, 0),
        {
          ...heads,
          files: paths.map((path, at) => newFile(path, starts[at] ?? 0, sizes[at] ?? 0)),
          tree: {
            rows: [
              folderRow("Doc", "Doc", 0, 2),
              fileRow(0, "whatsnew.rst", 1),
              folderRow("Lib", "Lib", 0, 5),
              fileRow(1, "ast.py", 1, { threads: 2 }),
              fileRow(2, "os.py", 1),
              fileRow(3, "setup.py", 0),
            ],
            folded: false,
          },
        },
      ),
    ),
    pullRequestRows: ok(paths.flatMap((path, at) => fileRows(path, sizes[at] ?? 0, `first of ${path}`))),
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "#7 Issue 7" })).toBeVisible();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  const pane = page.getByRole("region", { name: "PR view" });
  await expect(pane).toContainText("The fix");

  // The rail opens on the Conversation, where the tree reads the diff.
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("f");
  const rail = pane.getByRole("complementary", { name: "Rail" });
  const tree = rail.getByRole("tree", { name: "Changed files" });
  await expect(tree.getByRole("treeitem")).toHaveText([
    "▾Doc",
    "whatsnew.rst+40−0",
    "▾Lib",
    "ast.py2+98−0",
    "os.py+40−0",
    "setup.py+3−0",
  ]);
  // A click on a file goes to its header on the Files tab, and marks it.
  await tree.getByRole("treeitem", { name: /os\.py/ }).click();
  const diff = pane.getByRole("region", { name: "Diff" });
  await expect(pane.locator("#pr-tab-files")).toHaveAttribute("aria-selected", "true");
  await expect.poll(() => headerBelowTop(diff, "Lib/os.py")).toBe(0);
  const current = tree.locator('[aria-current="true"]');
  await expect(current).toHaveText("os.py+40−0");
  // The mark follows the diff as it scrolls.
  await diff.focus();
  await page.keyboard.press("Alt+PageUp");
  await expect.poll(() => headerBelowTop(diff, "Lib/ast.py")).toBe(0);
  await expect(current).toHaveText("ast.py2+98−0");
  // A short file at the end is marked, though its header can't reach the top, which shows the file before.
  await tree.getByRole("treeitem", { name: /setup\.py/ }).click();
  await expect.poll(() => headerBelowTop(diff, "setup.py")).toBeGreaterThan(100);
  await expect(current).toHaveText("setup.py+3−0");
  // Side by side and back, and one file at a time and back, leave the tab at that file.
  await diff.focus();
  await page.keyboard.press("ControlOrMeta+Alt+u");
  await page.keyboard.press("ControlOrMeta+Alt+u");
  const bar = pane.getByRole("navigation", { name: "File on show" });
  for (let round = 0; round < 2; round += 1) {
    await page.keyboard.press("ControlOrMeta+k");
    await page.keyboard.press("o");
    await expect(bar).toContainText("4 of 4");
    await page.keyboard.press("ControlOrMeta+k");
    await page.keyboard.press("o");
    await expect(bar).toBeHidden();
    await expect(current).toHaveText("setup.py+3−0");
  }
  // The file before goes on from the file marked.
  await page.keyboard.press("Alt+PageUp");
  await expect.poll(() => headerBelowTop(diff, "Lib/os.py")).toBe(0);
  await expect(current).toHaveText("os.py+40−0");
  // The next file is the short one again, past which there is none, and the file before goes back.
  await page.keyboard.press("Alt+PageDown");
  await expect.poll(() => headerBelowTop(diff, "setup.py")).toBeGreaterThan(100);
  await expect(current).toHaveText("setup.py+3−0");
  await page.keyboard.press("Alt+PageDown");
  await expect(current).toHaveText("setup.py+3−0");
  await page.keyboard.press("Alt+PageUp");
  await expect.poll(() => headerBelowTop(diff, "Lib/os.py")).toBe(0);
  await expect(current).toHaveText("os.py+40−0");
  await page.keyboard.press("Alt+PageUp");
  await expect.poll(() => headerBelowTop(diff, "Lib/ast.py")).toBe(0);
  await expect(current).toHaveText("ast.py2+98−0");

  // ↓ ↑ move, ← folds, and Enter opens a file, one file at a time showing it alone.
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.press("o");
  await expect(bar).toContainText("2 of 4");
  await tree.focus();
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowLeft");
  await expect(tree.getByRole("treeitem")).toHaveText(["▾Doc", "whatsnew.rst+40−0", "▸Lib", "setup.py+3−0"]);
  await expect(current).toHaveText("▸Lib");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(bar).toContainText("1 of 4");
  await expect(current).toHaveText("whatsnew.rst+40−0");
  await expect(tree).toBeFocused();

  // The filter narrows the tree to the paths that hold what the user types, its folders open.
  await rail.getByRole("textbox", { name: "Filter the changed files" }).fill("os");
  await expect(tree.getByRole("treeitem")).toHaveText(["▾Lib", "os.py+40−0"]);
});
