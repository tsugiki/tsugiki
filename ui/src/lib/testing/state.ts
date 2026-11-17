// The helpers the tests share that reach into the app's bindings and state, kept apart from helpers.ts so a
// test that only presses a key doesn't load them.
import { vi } from "vitest";
import { events, type Hit } from "../data/bindings";
import { HitCode } from "../state/hit-code.svelte";
import type { Peek } from "../state/peek";

/** Catches the listener the state registers for a Repository's changes, so a test can say one changed as
 * the core would, and which items. The test mocks `$lib/data/bindings` with a `changed` event. */
export const catchChanged = () => {
  const stop = vi.fn();
  const send = vi.fn();
  vi.mocked(events.changed.listen).mockImplementation(async (callback) => {
    send.mockImplementation((repository: string, items: number[] = []) => {
      callback({ event: "changed", id: 1, payload: { repository, items } });
    });
    return stop;
  });
  return { stop, send };
};

/** A peek under row 2 that looks up `eggs` at the PR's head, at the stage `lookup` says. */
export const looking = (lookup: Peek["lookup"]): Peek => ({
  at: { kind: "row", row: 2 },
  layout: "inline",
  symbol: "eggs",
  side: "head",
  commit: "abcdef123",
  lookup,
});

/** A hit of a lookup in `path` at line `line` of the PR's head. */
export const hitIn = (path: string, line: number): Hit => ({
  place: { kind: "repository", path, side: "head", commit: "fedcba987" },
  line,
  column: 4,
  text: `    def area${String(line)}(self):`,
});

/** A peek that found `hits` and shows the code of the one at `index`. */
export const showing = (hits: Hit[], index: number): Peek => {
  const hit = hits[index];
  if (hit === undefined) throw new Error("no such hit");
  return {
    ...looking({ state: "answered", answer: { state: "found", hits } }),
    code: new HitCode("python/cpython", hit, index),
  };
};
