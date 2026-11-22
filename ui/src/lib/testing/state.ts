// The helpers the tests share that reach into the app's bindings and state, kept apart from helpers.ts so a
// test that only presses a key doesn't load them.
import { vi } from "vitest";
import {
  events,
  type AnsweredMark_Deserialize as AnsweredMark,
  type Hit,
  type MarkKept_Deserialize as MarkKept,
} from "../data/bindings";
import type { Channel } from "../data/channel";
import { done } from "./fixtures";
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

/** What the core answers a mark the window waits on (#489), by its kind: `{ status: "ok" }` once the store
 * kept it, or why it couldn't. A test sets each to resolve at once, or to wait on a promise it holds. */
type MarkAnswers = Record<
  AnsweredMark["kind"],
  (repository: string, mark: AnsweredMark) => Promise<MarkKept>
>;

/** A stand-in for `commands.markAnswered`, whose test mocks `$lib/data/channel` with a class that has an
 * `onmessage`. It takes each mark at once, as the core does, and once the answer `answers` holds for its
 * kind settles, sends that on the mark's channel. */
export const answeringMarks =
  (answers: MarkAnswers) => (repository: string, mark: AnsweredMark, kept: Channel<MarkKept>) => {
    void answers[mark.kind](repository, mark).then((answer) => {
      kept.onmessage(answer);
    });
    return Promise.resolve(done);
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

/** A peek that found `hits` and shows the code of the one at `index`, which a test names among them. */
export const showing = (hits: Hit[], index: number): Peek => ({
  ...looking({ state: "answered", answer: { state: "found", hits } }),
  code: new HitCode("python/cpython", hits[index]!, index),
});
