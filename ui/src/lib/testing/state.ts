// The helpers the tests share that reach into the app's bindings and state, kept apart from helpers.ts so a
// test that only presses a key doesn't load them.
import { vi } from "vitest";
import { events, type Acted_Serialize, type Hit } from "../data/bindings";
import { HitCode } from "../state/hit-code.svelte";
import { browserClock, HudState, type HudCore } from "../state/hud.svelte";
import type { Peek } from "../state/peek";
import type { DarkQuery } from "../state/theme.svelte";

/** A HUD on a fixed clock that asks for no real frame, while cancelling a frame and reporting a paint stay
 * the browser's. */
export const testHud = (core: HudCore) =>
  new HudState({ ...browserClock, now: () => 0, requestFrame: () => 1 }, core);

/** The system on its light side, which says nothing when it changes, for a `ThemeState`. */
export const LIGHT_SIDE = { matches: false, addEventListener: () => {} } as unknown as DarkQuery;

/** Catches the listener the state registers for a Repository's changes, so a test can say one changed as
 * the core would, and which items, and the listener it registers for the answers to its actions, so a test
 * can answer one as the core would (#1052). The test mocks `$lib/data/bindings` with `changed` and `acted`
 * events. */
export const catchChanged = () => {
  const stop = vi.fn();
  const send = vi.fn();
  const answer = vi.fn();
  vi.mocked(events.changed.listen).mockImplementation(async (callback) => {
    send.mockImplementation((repository: string, items: number[] = []) => {
      callback({ event: "changed", id: 1, payload: { repository, items } });
    });
    return stop;
  });
  vi.mocked(events.acted.listen).mockImplementation(async (callback) => {
    answer.mockImplementation((payload: Acted_Serialize) => {
      callback({ event: "acted", id: 1, payload });
    });
    return stop;
  });
  return { stop, send, answer };
};

/** A peek under row 2 that looks up `eggs` at the PR's head, at the stage `lookup` says. */
export const looking = (lookup: Peek["lookup"]): Peek => ({
  at: { kind: "row", row: 2 },
  layout: "inline",
  symbol: "eggs",
  side: "head",
  commit: "abcdef123",
  column: null,
  lookup,
});

/** A hit of a lookup in `path` at line `line` of the PR's head. */
export const hitIn = (path: string, line: number): Hit => ({
  place: { kind: "repository", path, side: "head", commit: "fedcba987" },
  line,
  column: 4,
  text: `    def area${String(line)}(self):`,
});

/** A peek that found `hits` and shows the code of the one at `index`, which a test names among them. */
export const showing = (hits: Hit[], index: number): Peek => ({
  ...looking({ state: "answered", answer: { state: "found", hits } }),
  code: new HitCode("python/cpython", hits[index]!, index),
});
