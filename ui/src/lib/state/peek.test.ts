import { expect, it } from "vitest";
import type { Hit, ServerName } from "$lib/data/bindings";
import type { LaidThread, ThreadAnchor } from "./diff-layout";
import { HitCode } from "./hit-code.svelte";
import { PEEK_ID, PEEK_ROWS, peekPlaces, peekWords, withPeek, type Peek } from "./peek";

const laid = <At extends ThreadAnchor>(id: string, at: At) => ({ id, at, places: 3 });
const ids = (threads: LaidThread[]) => threads.map((thread) => thread.id);

const threads = [
  laid("row 2", { kind: "row", row: 2 }),
  laid("file 0 line 4", { kind: "line", file: 0, line: 4 }),
  laid("row 5", { kind: "row", row: 5 }),
  laid("file 1 line 9", { kind: "line", file: 1, line: 9 }),
  laid("end", { kind: "end" }),
];

it("puts the peek under a row after the threads above it and before those under it or below", () => {
  const under = (row: number) => ids(withPeek(threads, laid(PEEK_ID, { kind: "row", row })));

  expect(under(1).indexOf(PEEK_ID)).toBe(0);
  expect(under(2).indexOf(PEEK_ID)).toBe(0);
  expect(under(3).indexOf(PEEK_ID)).toBe(2);
  expect(under(9).indexOf(PEEK_ID)).toBe(5);
});

it("puts the peek under a line among the threads under lines of its own file", () => {
  const under = (file: number, line: number) =>
    ids(withPeek(threads, laid(PEEK_ID, { kind: "line", file, line })));

  expect(under(0, 4).indexOf(PEEK_ID)).toBe(1);
  expect(under(0, 7).indexOf(PEEK_ID)).toBe(5);
  expect(under(1, 2).indexOf(PEEK_ID)).toBe(3);
});

const looking = (lookup: Peek["lookup"]): Peek => ({
  at: { kind: "row", row: 2 },
  symbol: "eggs",
  side: "head",
  commit: "abcdef123",
  lookup,
});

it("says what a lookup waits for, with the seconds once they pass one", () => {
  const body = (lookup: Peek["lookup"], now = 10_000) => peekWords(looking(lookup), now).body;
  const asking = { step: "asking", server: "ty" } as const;

  expect(body({ state: "looking" })).toBe("Searching for the definition of eggs…");
  expect(body({ state: "looking", waiting: { step: "starting", server: "ty" }, since: 9000 })).toBe(
    "Starting ty…",
  );
  expect(body({ state: "looking", waiting: asking, since: 9500 })).toBe(
    "Asking ty for the definition of eggs…",
  );
  expect(body({ state: "looking", waiting: asking, since: 6500 })).toBe(
    "Asking ty for the definition of eggs… 3 s",
  );
  expect(body({ state: "looking", waiting: asking })).toBe("Asking ty for the definition of eggs…");
  const rust = { step: "starting", server: "rust-analyzer" } as const;
  expect(body({ state: "looking", waiting: rust })).toBe("Starting rust-analyzer…");
});

it("says how to set up the server for the file's language", () => {
  const notSetUp = (server: ServerName) =>
    peekWords(looking({ state: "answered", answer: { state: "not_set_up", server } })).body;

  expect(notSetUp("ty")).toBe("Set up ty to go to definitions in Python");
  expect(notSetUp("rust-analyzer")).toBe("Set up rust-analyzer to go to definitions in Rust");
  expect(notSetUp("clangd")).toBe("Set up clangd to go to definitions in C");
});

it("says a lookup was cancelled, in a peek with words only", () => {
  const cancelled = looking({ state: "answered", answer: { state: "cancelled" } });

  expect(peekWords(cancelled).body).toBe("The lookup was cancelled");
});

/** A hit in `path` at line `line` at the PR's head. */
const hitIn = (path: string, line: number): Hit => ({
  place: { kind: "repository", path, side: "head", commit: "fedcba987" },
  line,
  column: 4,
  text: `def area${String(line)}():`,
});

/** A peek with `hits` that shows the code of the one at `index`. */
const showing = (hits: Hit[], index: number): Peek => {
  const hit = hits[index];
  if (hit === undefined) throw new Error("no such hit");
  return {
    ...looking({ state: "answered", answer: { state: "found", hits } }),
    code: new HitCode("python/cpython", hit, index),
  };
};

it("names the hit the peek shows in its header, and counts it among the hits", () => {
  const hits = [hitIn("Lib/a.py", 3), hitIn("Lib/b.py", 9), hitIn("Lib/c.py", 12)];

  expect(peekWords(showing(hits, 0))).toMatchObject({ where: "Lib/a.py:3", count: "1 of 3" });
  expect(peekWords(showing(hits, 2))).toMatchObject({
    found: { hits, shown: 2, hit: hits[2] },
    where: "Lib/c.py:12",
    commit: "at fedcba9 · the PR's head",
    count: "3 of 3",
    body: "def area12():",
  });
});

it("keeps its rows while it lists several hits, even when the hit's file can't be read", () => {
  const unreadable = (hits: Hit[]) => {
    const peek = showing(hits, 0);
    if (peek.code) peek.code.problem = "Tsugiki couldn't read Lib/a.py";
    return peek;
  };

  expect(peekPlaces(showing([hitIn("Lib/a.py", 3)], 0))).toBe(PEEK_ROWS);
  expect(peekPlaces(unreadable([hitIn("Lib/a.py", 3)]))).toBe(2);
  expect(peekPlaces(unreadable([hitIn("Lib/a.py", 3), hitIn("Lib/b.py", 9)]))).toBe(PEEK_ROWS);
});

it("leaves the threads in their order", () => {
  const placed = withPeek(threads, laid(PEEK_ID, { kind: "row", row: 3 }));

  expect(ids(placed).filter((id) => id !== PEEK_ID)).toEqual(ids(threads));
});

const declaration = {
  place: { kind: "repository", path: "Include/spam.h", side: "base", commit: "base123" },
  line: 3,
  column: 4,
  text: "int spam_count(int limit);",
} as const;

it("says under an answer that came while clangd indexes how far it has got, and nothing once it is built", () => {
  const answered = (hits: Hit[], indexing?: Peek["indexing"]): Peek => ({
    ...looking({ state: "answered", answer: { state: "found", hits } }),
    ...(indexing && { indexing }),
  });
  const counted = { state: "building", server: "clangd", files: { done: 120, total: 452 } } as const;
  const uncounted = { state: "building", server: "clangd", files: null } as const;

  expect(peekWords(answered([declaration], counted)).indexing).toBe(
    "clangd is indexing (120 of 452 files): this may be a declaration",
  );
  expect(peekWords(answered([declaration], uncounted)).indexing).toBe(
    "clangd is indexing: this may be a declaration",
  );
  expect(peekWords(answered([declaration])).indexing).toBeUndefined();
  // A peek with words only takes a row more for the line.
  expect(peekPlaces(answered([], counted))).toBe(3);
  expect(peekPlaces(answered([]))).toBe(2);
  expect(peekPlaces(answered([declaration], counted))).toBe(PEEK_ROWS);
});
