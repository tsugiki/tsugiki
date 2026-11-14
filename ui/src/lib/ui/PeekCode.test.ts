import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { HitCode } from "$lib/state/hit-code.svelte";
import PeekCode from "./PeekCode.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { definitionCode: () => new Promise(() => {}) },
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
