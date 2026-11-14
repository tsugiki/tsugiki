import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { HitCode } from "$lib/state/hit-code.svelte";
import PeekCode from "./PeekCode.svelte";

const core = vi.hoisted(() => ({
  definitionCode: vi.fn(() => new Promise(() => {})),
  definitionSpans: vi.fn(() => new Promise(() => {})),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { definitionCode: core.definitionCode, definitionSpans: core.definitionSpans },
}));

beforeEach(() => {
  // jsdom lays nothing out, so bind:clientHeight has nothing to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

const hit = {
  place: { kind: "repository" as const, path: "Lib/a.py", side: "head" as const, commit: "abc" },
  line: 3,
  column: 4,
  text: "def area(self):",
};

it("scrolls to each new hit's line, and back to where the user left a hit's code", async () => {
  const far = new HitCode("python/cpython", { ...hit, line: 900 }, 0);
  const shown = render(PeekCode, { code: far, line: 900 });
  const region = screen.getByRole("region", { name: "Definition's code" });
  // The hit's line sits three rows from the top.
  expect(region.scrollTop).toBe((900 - 1 - 3) * 20);

  await shown.rerender({ code: new HitCode("python/cpython", { ...hit, line: 40 }, 1), line: 40 });
  expect(region.scrollTop).toBe((40 - 1 - 3) * 20);

  far.scrollTop = 500;
  await shown.rerender({ code: far, line: 900 });
  expect(region.scrollTop).toBe(500);
});

it("paints each line's spans in the theme's syntax colours once they come", async () => {
  const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
  core.definitionCode.mockReturnValueOnce(ok({ count: 3, lines: ["x = 1", "", "def area(self):"] }));
  const keyword = { start: 0, end: 3, highlight: "keyword" };
  const area = { start: 4, end: 8, highlight: "function" };
  core.definitionSpans.mockReturnValueOnce(ok([[], [], [keyword, area]]));
  const code = new HitCode("python/cpython", hit, 0);
  render(PeekCode, { code, line: 3 });

  const hitLine = await vi.waitFor(() => {
    const line = document.querySelector(".hit .text");
    expect(line?.querySelector(".hl-function")).not.toBeNull();
    return line;
  });

  const painted = [...(hitLine?.querySelectorAll("span") ?? [])].map((span) => [
    span.classList[0],
    span.textContent,
  ]);
  expect(painted).toEqual([
    ["hl-keyword", "def"],
    ["hl-function", "area"],
  ]);
  expect(hitLine?.textContent).toBe("def area(self):");
});

/** Clicks `element` with the caret `offset` characters into its text, where Chromium leaves a collapsed
 * selection after a click; jsdom lays nothing out to find the caret from. */
const clickAt = async (element: Element, offset: number, init: MouseEventInit = {}) => {
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  let left = offset;
  let node = walker.nextNode() as Text | null;
  while (node !== null && left > node.length) {
    left -= node.length;
    node = walker.nextNode() as Text | null;
  }
  getSelection()?.collapse(node ?? element, node === null ? 0 : left);
  await fireEvent.click(element, init);
};

/** The code of the file's line `line`, once it has come. */
const codeOf = async (line: number) =>
  vi.waitFor(() => {
    const code = document.querySelector(`[data-code-line="${String(line)}"] .text`);
    expect(code?.textContent).not.toBe("");
    return code as Element;
  });

/** The pieces of the word picked on each line, joined up around the caret. */
const picked = () =>
  [...document.querySelectorAll(".text")]
    .filter((code) => code.querySelector(".picked") !== null)
    .map((code) => [...code.querySelectorAll(".picked")].map((piece) => piece.textContent).join(""));

/** A peek's code of `hit`'s file, which reads `x = width` and `def area(self):`, with its keyword and its
 * function's name highlighted. */
const pickableCode = (pickable = true) => {
  const ok = <T>(data: T) => Promise.resolve({ status: "ok" as const, data });
  core.definitionCode.mockReturnValueOnce(ok({ count: 3, lines: ["x = width", "", "def area(self):"] }));
  const spans = [
    { start: 0, end: 3, highlight: "keyword" },
    { start: 4, end: 8, highlight: "function" },
  ];
  core.definitionSpans.mockReturnValueOnce(ok([[], [], spans]));
  const code = new HitCode("python/cpython", hit, 0);
  render(PeekCode, { code, line: 3, pickable });
  return code;
};

it("puts the caret on the word a click lands on, highlighted or not, and drops it on a line's number", async () => {
  const code = pickableCode();
  const area = await codeOf(3);
  await vi.waitFor(() => {
    expect(area.querySelector(".hl-function")).not.toBeNull();
  });

  await clickAt(area, 6);
  const highlighted = [
    picked(),
    area.querySelector(".picked")?.classList.contains("hl-function"),
    area.querySelectorAll(".caret").length,
    area.textContent,
  ];
  await clickAt(await codeOf(1), 6);
  const plain = [picked(), code.pick];
  await clickAt(document.querySelector('[data-code-line="1"] .number') as Element, 0);

  expect(highlighted).toEqual([["area"], true, 1, "def area(self):"]);
  expect(plain).toEqual([["width"], { line: 1, word: "width", start: 4, end: 9, caret: 6 }]);
  expect([picked(), code.pick]).toEqual([[], undefined]);
});

it("keeps the pick through a drag, which selects text to copy", async () => {
  const code = pickableCode();
  const area = await codeOf(3);
  await clickAt(area, 6);

  getSelection()?.selectAllChildren(area);
  await fireEvent.click(area);

  expect(code.pick?.word).toBe("area");
});

it("drops the pick on a click that leaves the caret on the code's box, or leaves no caret", async () => {
  const code = pickableCode();
  const area = await codeOf(3);
  const region = screen.getByRole("region", { name: "Definition's code" });
  const picks = [];

  for (const leave of [() => getSelection()?.collapse(region, 0), () => getSelection()?.removeAllRanges()]) {
    await clickAt(area, 6);
    picks.push(code.pick?.word);
    leave();
    await fireEvent.click(region);
    picks.push(code.pick);
  }

  expect(picks).toEqual(["area", undefined, "area", undefined]);
});

it("selects the word a double-click's first click picked, however far the browser's own word runs", async () => {
  const code = pickableCode();
  const area = await codeOf(3);

  await clickAt(area, 6);
  getSelection()?.selectAllChildren(area);
  await fireEvent.click(area, { detail: 2 });
  await fireEvent.dblClick(area, { detail: 2 });

  await vi.waitFor(() => {
    expect(getSelection()?.toString()).toBe("area");
  });
  expect(code.pick).toMatchObject({ word: "area", caret: null });
  expect(area.querySelectorAll(".caret")).toHaveLength(0);
});

it("selects nothing on a double-click whose word went before it came, or whose first click picked none", async () => {
  const code = pickableCode();
  const area = await codeOf(3);
  await clickAt(area, 6);
  code.pick = undefined;
  getSelection()?.removeAllRanges();

  await fireEvent.dblClick(area, { detail: 2 });
  await fireEvent.dblClick(area, { detail: 2 });
  await tick();

  expect([code.pick, getSelection()?.toString()]).toEqual([undefined, ""]);
});

it("picks no word in the file view, which has nothing to look it up with", async () => {
  const code = pickableCode(false);
  const area = await codeOf(3);
  code.pickWord(3, 6, false);

  await clickAt(area, 1);
  await fireEvent.dblClick(area, { detail: 2 });

  expect(code.pick?.word).toBe("area");
  expect(picked()).toEqual([]);
});

it("keeps the focus on the code when its file turns out unreadable, so the keys stay with it", async () => {
  const code = new HitCode("python/cpython", hit, 0);
  render(PeekCode, { code, line: 3 });
  const region = screen.getByRole("region", { name: "Definition's code" });
  region.focus();

  code.problem = "Tsugiki couldn't read Lib/a.py";
  await tick();

  expect(screen.getByRole("alert").textContent).toBe("Tsugiki couldn't read Lib/a.py");
  expect(region.isConnected).toBe(true);
  expect(document.activeElement).toBe(region);
});
