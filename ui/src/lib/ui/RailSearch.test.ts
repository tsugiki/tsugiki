import { cleanup, fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { PrViewState } from "$lib/state/pr-view.svelte";
import {
  type FileHits,
  type LineHit,
  type SearchState,
  TextSearchState,
} from "$lib/state/text-search.svelte";
import { ok } from "$lib/testing/fixtures";
import { pressOnFocus } from "$lib/testing/helpers";
import RailSearch from "./RailSearch.svelte";

const core = vi.hoisted(() => ({
  textSearch: vi.fn(),
  textSearchState: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    textSearch: core.textSearch,
    textSearchState: core.textSearchState,
    textSearchCancel: () => Promise.resolve(null),
  },
  events: { textSearchChanged: { listen: core.listen } },
}));

/** The ends of the diff the tab searches. */
const ENDS = { base: "b0", head: "abc" };

const hit = (
  line: number,
  text: string,
  ranges: [number, number][],
  cut: LineHit["cut"] = "neither",
): LineHit => ({
  line,
  text,
  cut,
  ranges: ranges.map(([start, end]) => ({ start, end })),
});

const found = (...files: FileHits[]): SearchState => ({
  state: "results",
  id: 1,
  files,
  matches: files.reduce((sum, file) => sum + file.lines.reduce((n, line) => n + line.ranges.length, 0), 0),
  end: "done",
});

/** The PR view the tab sits in, with the diff at `ENDS` or none ready yet, and the matches it opened. */
const prView = (ready = true) => {
  const files = { view: ready ? { state: "ready", ...ENDS } : { state: "fetching" }, error: undefined };
  const pr = {
    search: new TextSearchState("python/cpython"),
    files,
    loadFiles: vi.fn(),
    openMatch: vi.fn(() => Promise.resolve()),
  };
  return pr;
};

/** The tab in `pr`, with `state` what the core found, once it has shown it for a query. */
const shown = async (state: SearchState, pr = prView()) => {
  core.textSearchState.mockResolvedValue(state);
  render(RailSearch, { pr: pr as unknown as PrViewState });
  pr.search.seed("spam");
  await vi.waitFor(() => {
    expect(screen.queryByRole("tree")).not.toBeNull();
  });
  return pr;
};

const box = () => screen.getByRole("textbox", { name: "Search the PR's code" });
const tree = () => screen.getByRole("tree", { name: "Search results" });
const items = () => within(tree()).getAllByRole("treeitem");
const cursorText = () =>
  within(tree()).getByRole("treeitem", { selected: true }).textContent?.replace(/\s+/g, "");

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockResolvedValue(() => {});
  core.textSearch.mockResolvedValue(ok({ kind: "running", id: 1 }));
});

afterEach(() => {
  cleanup();
});

it("reads the PR's diff, waits for it, then searches and shows the results grouped by file, matches marked", async () => {
  const pr = prView(false);
  core.textSearchState.mockResolvedValue(
    found(
      {
        path: "Lib/os.py",
        generated: false,
        lines: [hit(3, "spam = 1", [[0, 4]]), hit(9, "eggs spam", [[5, 9]])],
      },
      { path: "Python/gen.c.h", generated: true, lines: [hit(1, "a spam b", [[2, 6]], "both")] },
    ),
  );
  render(RailSearch, { pr: pr as unknown as PrViewState });
  pr.search.seed("spam");
  await tick();
  const waiting = document.querySelector(".summary")?.textContent;

  pr.files.view = { state: "ready", ...ENDS };
  // The view isn't reactive here, so the tab hears the diff as PrDetail's would.
  pr.search.follow({ kind: "ready", ...ENDS });
  await vi.waitFor(() => {
    expect(screen.queryByRole("tree")).not.toBeNull();
  });

  expect(pr.loadFiles).toHaveBeenCalled();
  expect(waiting).toBe("Waiting for the checkout…");
  expect(screen.getByText("3 results in 2 files")).toBeDefined();
  expect(items().map((item) => item.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "▾ Lib/os.py 2",
    "3 spam = 1",
    "9 eggs spam",
    "▾ Python/gen.c.h Generated 1",
    "1 …a spam b…",
  ]);
  expect([...tree().querySelectorAll("mark")].map((mark) => mark.textContent)).toEqual([
    "spam",
    "spam",
    "spam",
  ]);
  expect(items()[3]?.classList.contains("generated")).toBe(true);
  expect(items()[0]?.getAttribute("aria-expanded")).toBe("true");
});

it("shows a line cut at either end alone with an ellipsis there", async () => {
  await shown(
    found({
      path: "a.py",
      generated: false,
      lines: [hit(1, "spam tail", [[0, 4]], "start"), hit(2, "head spam", [[5, 9]], "end")],
    }),
  );

  expect(items().map((item) => item.textContent?.replace(/\s+/g, " ").trim())).toEqual([
    "▾ a.py 2",
    "1 …spam tail",
    "2 head spam…",
  ]);
});

it("toggles match case, whole word and regex on their buttons and on Alt+C, Alt+W and Alt+R in the box", async () => {
  const pr = await shown(found({ path: "a.py", generated: false, lines: [hit(1, "spam", [[0, 4]])] }));
  const buttons = ["Match Case", "Match Whole Word", "Use Regular Expression"].map((name) =>
    screen.getByRole("button", { name }),
  );
  const titles = buttons.map((button) => button.title);

  await fireEvent.click(buttons[0] as HTMLElement);
  await fireEvent.keyDown(box(), { key: "w", code: "KeyW", altKey: true });
  await fireEvent.keyDown(box(), { key: "r", code: "KeyR", altKey: true });
  await fireEvent.keyDown(box(), { key: "c", code: "KeyC", altKey: true });
  const after = buttons.map((button) => button.getAttribute("aria-pressed"));

  expect(titles).toEqual([
    "Match Case (Alt+C)",
    "Match Whole Word (Alt+W)",
    "Use Regular Expression (Alt+R)",
  ]);
  expect(after).toEqual(["false", "true", "true"]);
  expect([pr.search.matchCase, pr.search.wholeWord, pr.search.regex]).toEqual([false, true, true]);
});

it("searches as the user types, and focuses the box with its text selected when asked", async () => {
  core.textSearchState.mockResolvedValue(found());
  const pr = prView();
  render(RailSearch, { pr: pr as unknown as PrViewState });

  await fireEvent.input(box(), { target: { value: "eggs" } });
  pr.search.wantsFocus = true;
  await tick();

  expect(pr.search.text).toBe("eggs");
  expect(document.activeElement).toBe(box());
  expect(pr.search.wantsFocus).toBe(false);
  await vi.waitFor(() => {
    expect(screen.getByText("No results")).toBeDefined();
  });
});

it("goes from the box to the list on ↓, where the keys move, fold, unfold and open, and go no further", async () => {
  const pr = await shown(
    found(
      { path: "a.py", generated: false, lines: [hit(1, "spam", [[0, 4]]), hit(4, "spam", [[0, 4]])] },
      { path: "b.py", generated: false, lines: [hit(2, "spam", [[0, 4]])] },
    ),
  );
  const heard = vi.fn();
  window.addEventListener("keydown", heard);
  box().focus();

  await pressOnFocus("a");
  await pressOnFocus("ArrowDown");
  const inList = document.activeElement === tree();
  await pressOnFocus("ArrowDown");
  await pressOnFocus("ArrowDown");
  const second = cursorText();
  await pressOnFocus("Enter");
  await pressOnFocus("ArrowLeft");
  await pressOnFocus("ArrowLeft");
  const folded = items().length;
  await pressOnFocus("ArrowRight");
  await pressOnFocus("ArrowRight");
  const intoFile = cursorText();
  await pressOnFocus("End");
  const last = cursorText();
  await pressOnFocus("Home");
  await pressOnFocus("PageDown");
  const paged = cursorText();
  await pressOnFocus("PageUp");
  await pressOnFocus("ArrowUp");
  await pressOnFocus("Enter");
  const foldedByEnter = items().length;
  await pressOnFocus("x");
  await pressOnFocus("Shift");
  window.removeEventListener("keydown", heard);

  expect([inList, second, folded, intoFile, last, paged, foldedByEnter]).toEqual([
    true,
    "4spam",
    3,
    "1spam",
    "2spam",
    "1spam",
    3,
  ]);
  expect(pr.openMatch.mock.calls).toEqual([[{ path: "a.py", line: 4, ends: ENDS }]]);
  // Only the keys the list doesn't use, and the one the box doesn't, go on.
  expect(heard).toHaveBeenCalledTimes(3);
  expect(tree().getAttribute("aria-activedescendant")).toBe(items()[0]?.id);
});

it("stays in the box on ↓ while there are no results", async () => {
  core.textSearchState.mockResolvedValue(found());
  const pr = prView();
  render(RailSearch, { pr: pr as unknown as PrViewState });
  box().focus();

  await fireEvent.keyDown(box(), { key: "ArrowDown" });

  expect(document.activeElement).toBe(box());
});

it("opens a match on a click, and folds a file on a click on its row", async () => {
  const pr = await shown(
    found({ path: "a.py", generated: false, lines: [hit(1, "spam", [[0, 4]]), hit(7, "spam", [[0, 4]])] }),
  );

  await fireEvent.click(items()[2] as HTMLElement);
  const cursor = cursorText();
  await fireEvent.click(items()[0] as HTMLElement);

  expect(pr.openMatch.mock.calls).toEqual([[{ path: "a.py", line: 7, ends: ENDS }]]);
  expect(cursor).toBe("7spam");
  expect(items().map((item) => item.getAttribute("aria-expanded"))).toEqual(["false"]);
});

it("draws only the rows near what shows of 5,000 results, and scrolls the list to the cursor", async () => {
  const lines = Array.from({ length: 5000 }, (_, i) => hit(i + 1, "spam", [[0, 4]]));
  await shown(found({ path: "big.c", generated: false, lines }));
  const list = tree();
  Object.defineProperty(list, "clientHeight", { configurable: true, value: 220 });
  const drawnAtTop = items().length;
  list.focus();

  await fireEvent.keyDown(list, { key: "End" });
  await tick();
  const atEnd = [
    list.scrollTop,
    items().some((item) => item.textContent?.replace(/\s+/g, "") === "5000spam"),
  ];
  // The wheel or the scroll bar draws the rows it scrolls to, and the list names no row under the cursor
  // while it draws none, above or below.
  list.scrollTop = 0;
  await fireEvent.scroll(list);
  const cursorBelow = list.getAttribute("aria-activedescendant");
  await fireEvent.keyDown(list, { key: "Home" });
  await tick();
  const home = list.scrollTop;
  list.scrollTop = 2200;
  await fireEvent.scroll(list);
  const scrolled = items().some((item) => item.textContent?.replace(/\s+/g, "") === "100spam");

  expect(drawnAtTop).toBeLessThan(30);
  expect([home, scrolled, cursorBelow, list.getAttribute("aria-activedescendant")]).toEqual([
    0,
    true,
    null,
    null,
  ]);
  expect(atEnd).toEqual([5001 * 22 - 220, true]);
  expect((list.firstElementChild as HTMLElement).style.height).toBe(`${String(5001 * 22)}px`);
});

it("puts the list back at its top for a new query's results, and when it draws the list anew", async () => {
  const lines = Array.from({ length: 5000 }, (_, i) => hit(i + 1, "spam", [[0, 4]]));
  const big = found({ path: "big.c", generated: false, lines });
  const pr = await shown(big);
  const scrolled = async () => {
    tree().scrollTop = 2200;
    await fireEvent.scroll(tree());
  };
  const top = () => [
    tree().scrollTop,
    items().some((item) => item.textContent?.replace(/\s+/g, "") === "1spam"),
  ];

  await scrolled();
  pr.search.seed("eggs");
  await vi.waitFor(() => {
    expect(pr.search.answered).toBe(2);
  });
  await tick();
  const newQuery = top();
  await scrolled();
  core.textSearchState.mockResolvedValue(found());
  pr.search.seed("ham");
  await vi.waitFor(() => {
    expect(screen.queryByRole("tree")).toBeNull();
  });
  core.textSearchState.mockResolvedValue(big);
  pr.search.seed("spam");
  await vi.waitFor(() => {
    expect(screen.queryByRole("tree")).not.toBeNull();
  });
  await tick();

  expect([newQuery, top()]).toEqual([
    [0, true],
    [0, true],
  ]);
});

it("marks the line under the box as trouble when the query can't run", async () => {
  core.textSearch.mockResolvedValue(ok({ kind: "bad_query", message: "unclosed group" }));
  const pr = prView();
  render(RailSearch, { pr: pr as unknown as PrViewState });

  pr.search.seed("(");
  await vi.waitFor(() => {
    expect(screen.getByText("unclosed group").classList.contains("troubled")).toBe(true);
  });
});
