import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { HitCode } from "$lib/state/hit-code.svelte";
import type { Peek } from "$lib/state/peek";
import DefinitionPeek from "./DefinitionPeek.svelte";

/** A hit of a lookup, as the core gives it. */
type Hit = ConstructorParameters<typeof HitCode>[1];

vi.mock("$lib/data/bindings", () => ({
  commands: { definitionCode: () => new Promise(() => {}) },
}));

beforeEach(() => {
  // jsdom lays nothing out, so the code's bind:clientHeight has nothing to observe.
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
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const peekWith = (lookup: Peek["lookup"]): Peek => ({
  at: { kind: "row", row: 2 },
  symbol: "eggs",
  side: "head",
  commit: "abcdef123",
  lookup,
});

it("counts the seconds a lookup waits for ty's answer, and stops once it answers", async () => {
  vi.useFakeTimers({ now: 10_000 });
  const asking = peekWith({ state: "looking", waiting: { step: "asking", server: "ty" }, since: 10_000 });
  const shown = render(DefinitionPeek, {
    peek: asking,
    onclose: () => {},
    onhit: () => {},
    onopen: () => {},
  });
  const body = () => screen.getByText(/eggs…|No definition/).textContent;
  expect(body()).toBe("Asking ty for the definition of eggs…");

  await vi.advanceTimersByTimeAsync(3000);
  expect(body()).toBe("Asking ty for the definition of eggs… 3 s");

  await shown.rerender({ peek: peekWith({ state: "answered", answer: { state: "found", hits: [] } }) });
  await tick();
  expect(body()).toBe("No definition of eggs");
  expect(vi.getTimerCount()).toBe(0);
});

it("counts no seconds while ty starts, as the words say none", () => {
  vi.useFakeTimers({ now: 10_000 });
  const starting = peekWith({ state: "looking", waiting: { step: "starting", server: "ty" }, since: 10_000 });

  render(DefinitionPeek, { peek: starting, onclose: () => {}, onhit: () => {}, onopen: () => {} });

  expect(screen.getByText("Starting ty…")).toBeDefined();
  expect(vi.getTimerCount()).toBe(0);
});

it("shows a line under the header while clangd indexes, and takes it away once it has built its index", async () => {
  const answered = peekWith({ state: "answered", answer: { state: "found", hits: [] } });
  const files = { done: 120, total: 452 };
  const indexing = { ...answered, indexing: { state: "building", server: "clangd", files } } as const;
  const line = "clangd is indexing (120 of 452 files): this may be a declaration";

  const shown = render(DefinitionPeek, {
    peek: indexing,
    onclose: () => {},
    onhit: () => {},
    onopen: () => {},
  });
  const before = screen.queryByText(line);
  await shown.rerender({ peek: answered });

  expect(before).not.toBeNull();
  expect(screen.queryByText(line)).toBeNull();
  expect(screen.getByText("No definition of eggs")).toBeDefined();
});

/** A hit in `path` at line `line` of the PR's head. */
const hitIn = (path: string, line: number): Hit => ({
  place: { kind: "repository", path, side: "head", commit: "fedcba987" },
  line,
  column: 4,
  text: `    def area${String(line)}(self):`,
});

/** A peek with `hits` that shows the code of the one at `index`. */
const showing = (hits: Hit[], index: number): Peek => {
  const hit = hits[index];
  if (hit === undefined) throw new Error("no such hit");
  return {
    ...peekWith({ state: "answered", answer: { state: "found", hits } }),
    code: new HitCode("python/cpython", hit, index),
  };
};

const hits = [hitIn("Lib/a.py", 3), hitIn("Lib/b.py", 9), hitIn("Lib/c.py", 12)];

it("lists the hits on its right, each with its path, line and text, and marks the one it shows", async () => {
  const shown = render(DefinitionPeek, {
    peek: showing(hits, 1),
    onclose: () => {},
    onhit: () => {},
    onopen: () => {},
  });

  const list = screen.getByRole("listbox", { name: "Definitions" });
  const rows = within(list).getAllByRole("option");
  expect(rows.map((row) => row.textContent)).toEqual([
    "Lib/a.py:3 def area3(self):",
    "Lib/b.py:9 def area9(self):",
    "Lib/c.py:12 def area12(self):",
  ]);
  expect(rows.map((row) => row.getAttribute("aria-selected"))).toEqual(["false", "true", "false"]);
  // The focus stays on the list, which names the hit shown to a screen reader.
  expect(list.getAttribute("aria-activedescendant")).toBe(rows[1]?.id);
  expect(new Set(rows.map((row) => row.id)).size).toBe(3);
  expect(screen.getByText("2 of 3")).toBeDefined();
  expect(screen.getByText("Lib/b.py:9", { selector: ".header *" })).toBeDefined();

  await shown.rerender({ peek: showing(hits, 2) });
  expect(screen.getByText("3 of 3")).toBeDefined();
  expect(list.getAttribute("aria-activedescendant")).toBe(rows[2]?.id);
});

it("scrolls the list, and only the list, to keep the hit shown in sight", async () => {
  // The list shows 3 rows of 40 px.
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(function (this: HTMLElement) {
    return this.getAttribute("role") === "listbox" ? 120 : 0;
  });
  const many = Array.from({ length: 10 }, (_, at) => hitIn(`Lib/m${String(at)}.py`, at + 1));
  const scroller = document.createElement("div");
  document.body.append(scroller);
  const shown = render(DefinitionPeek, {
    target: scroller,
    props: { peek: showing(many, 5), onclose: () => {}, onhit: () => {}, onopen: () => {} },
  });
  const list = screen.getByRole("listbox");

  // Row 5 ends at 240 px, so the list scrolls until that end shows at its bottom.
  expect(list.scrollTop).toBe(120);
  await shown.rerender({ peek: showing(many, 4) });
  expect(list.scrollTop).toBe(120);
  await shown.rerender({ peek: showing(many, 1) });
  expect(list.scrollTop).toBe(40);
  expect(scroller.scrollTop).toBe(0);
  expect(document.documentElement.scrollTop).toBe(0);
});

it("lists a single hit too, beside the words that say its file can't be read", async () => {
  const peek = showing([hitIn("Lib/a.py", 3)], 0);
  render(DefinitionPeek, { peek, onclose: () => {}, onhit: () => {}, onopen: () => {} });

  expect(screen.getAllByRole("option").map((row) => row.textContent)).toEqual([
    "Lib/a.py:3 def area3(self):",
  ]);
  if (peek.code) peek.code.problem = "Tsugiki couldn't read Lib/a.py";
  await tick();
  expect(screen.getByRole("alert").textContent).toBe("Tsugiki couldn't read Lib/a.py");
  expect(screen.getAllByRole("option")).toHaveLength(1);
});

it("picks the hit a click lands on", async () => {
  const onhit = vi.fn();
  render(DefinitionPeek, { peek: showing(hits, 0), onclose: () => {}, onhit, onopen: () => {} });

  await fireEvent.click(screen.getByText("def area12(self):", { selector: "[role=option] *" }));

  expect(onhit).toHaveBeenCalledWith(2);
});

it("moves through the hits with ↑ and ↓ while it has the focus, and keeps them from the diff", async () => {
  const onhit = vi.fn();
  const diff = vi.fn<(e: KeyboardEvent) => void>();
  window.addEventListener("keydown", diff);
  render(DefinitionPeek, { peek: showing(hits, 1), onclose: () => {}, onhit, onopen: () => {} });

  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "ArrowDown" });
  await fireEvent.keyDown(screen.getByRole("region", { name: "Definition's code" }), { key: "ArrowUp" });
  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "ArrowDown", ctrlKey: true });
  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "Escape" });
  window.removeEventListener("keydown", diff);

  expect(onhit.mock.calls).toEqual([[2], [0]]);
  // Ctrl+↓ isn't the peek's, and the window's Escape closes the peek, so both go on to the window's keys.
  expect(diff.mock.calls.map(([e]) => e.key)).toEqual(["ArrowDown", "Escape"]);
});

it("goes to the hit it shows on Open, and on Enter or F12 while it has the focus, which the diff never hears", async () => {
  const onopen = vi.fn();
  const diff = vi.fn<(e: KeyboardEvent) => void>();
  window.addEventListener("keydown", diff);
  const props = { onclose: () => {}, onhit: () => {}, onopen };
  render(DefinitionPeek, { peek: showing(hits, 1), open: "ready", ...props });

  await fireEvent.click(screen.getByRole("button", { name: "Open" }));
  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "Enter" });
  await fireEvent.keyDown(screen.getByRole("region", { name: "Definition's code" }), { key: "F12" });
  // A held key would go again and again, and Alt+F12 is the diff's.
  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "F12", repeat: true });
  await fireEvent.keyDown(screen.getByRole("listbox"), { key: "F12", altKey: true });
  window.removeEventListener("keydown", diff);

  expect(onopen).toHaveBeenCalledTimes(3);
  expect(diff.mock.calls.map(([e]) => [e.key, e.altKey])).toEqual([["F12", true]]);
});

it("offers Open as the tab says, and waits while the tab finds the hit's row", async () => {
  const props = { onclose: () => {}, onhit: () => {}, onopen: () => {} };
  const shown = render(DefinitionPeek, { peek: showing(hits, 0), ...props });
  const nowhere = screen.queryByRole("button", { name: "Open" });
  await shown.rerender({ open: "ready" });
  const open = screen.getByRole<HTMLButtonElement>("button", { name: "Open" });
  const ready = open.disabled;
  await shown.rerender({ open: "going" });

  expect(nowhere).toBeNull();
  expect(ready).toBe(false);
  expect(open.disabled).toBe(true);
  expect(open.title).toBe("Go to this definition (Enter, F12)");
});
