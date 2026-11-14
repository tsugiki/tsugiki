import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import type { Peek } from "$lib/state/peek";
import DefinitionPeek from "./DefinitionPeek.svelte";

afterEach(() => {
  vi.useRealTimers();
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
  const shown = render(DefinitionPeek, { peek: asking, onclose: () => {} });
  const body = () => screen.getByText(/eggs…|No definition/).textContent;
  expect(body()).toBe("Asking ty for the definition of eggs…");

  await vi.advanceTimersByTimeAsync(3000);
  expect(body()).toBe("Asking ty for the definition of eggs… 3 s");

  await shown.rerender({ peek: peekWith({ state: "answered", answer: { state: "found", hits: [] } }) });
  await tick();
  expect(body()).toBe("No definition of eggs");
  expect(vi.getTimerCount()).toBe(0);
});
