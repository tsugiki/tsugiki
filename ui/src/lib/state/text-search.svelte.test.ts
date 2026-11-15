import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FileHits, LineHit, SearchState } from "$lib/data/bindings";
import { literalRegex, SEARCH_PAUSE, TextSearchState } from "./text-search.svelte";

const core = vi.hoisted(() => ({
  textSearch: vi.fn(),
  textSearchState: vi.fn(),
  listen: vi.fn(),
  /** Tells the window that what the core found may have changed. */
  changed: () => {},
  stop: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: { textSearch: core.textSearch, textSearchState: core.textSearchState },
  events: { textSearchChanged: { listen: core.listen } },
}));

const hit = (line: number, matches = 1): LineHit => ({
  line,
  text: "spam ".repeat(matches),
  cut: "neither",
  ranges: Array.from({ length: matches }, (_, i) => ({ start: i * 5, end: i * 5 + 4 })),
});
const fileHits = (path: string, ...lines: LineHit[]): FileHits => ({ path, generated: false, lines });
const results = (
  id: number,
  files: FileHits[],
  end: "running" | "done" | "stopped" = "done",
): Extract<SearchState, { state: "results" }> => ({
  state: "results",
  id,
  files,
  matches: files.reduce((sum, file) => sum + file.lines.reduce((n, line) => n + line.ranges.length, 0), 0),
  end,
});
const running = (id: number) => ({ status: "ok" as const, data: { kind: "running" as const, id } });

/** A search on the head `abc`, whose core answers each query with the next number and `state`. */
const ready = (state: SearchState = { state: "idle" }) => {
  let id = 0;
  core.textSearch.mockImplementation(() => {
    id += 1;
    return Promise.resolve(running(id));
  });
  core.textSearchState.mockResolvedValue(state);
  const search = new TextSearchState("python/cpython");
  search.follow("abc");
  return search;
};

const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockImplementation((heard: () => void) => {
    core.changed = heard;
    return Promise.resolve(core.stop);
  });
});

afterEach(() => {
  vi.useRealTimers();
});

it("runs the query once the keys pause, with the options off, and shows what the core found", async () => {
  vi.useFakeTimers();
  const search = ready(results(1, [fileHits("Lib/os.py", hit(3), hit(9, 2)), fileHits("a.py", hit(1))]));

  search.type("sp");
  search.type("spam");
  await vi.advanceTimersByTimeAsync(SEARCH_PAUSE - 1);
  const early = core.textSearch.mock.calls.length;
  await vi.advanceTimersByTimeAsync(1);

  expect(early).toBe(0);
  expect(core.textSearch.mock.calls).toEqual([
    ["python/cpython", { text: "spam", case: "ignored", words: "within", syntax: "literal" }],
  ]);
  expect(search.summary).toBe("4 results in 2 files");
  expect(search.troubled).toBe(false);
  expect(
    search.rows.map((row) => (row.kind === "file" ? [row.file.path, row.matches] : row.hit.line)),
  ).toEqual([["Lib/os.py", 3], 3, 9, ["a.py", 1], 1]);
});

it("runs at once on each toggle, and puts the picked text in the box as a regex with regex on", async () => {
  const search = ready();
  search.text = "spam";

  search.toggle("case");
  search.toggle("words");
  search.toggle("regex");
  search.seed("a.b(c)");
  await settled();
  search.toggle("regex");
  search.seed("a.b");
  await settled();

  const queries = core.textSearch.mock.calls.map((call: unknown[]) => call[1]);
  expect(queries.at(2)).toEqual({ text: "spam", case: "matched", words: "whole", syntax: "regex" });
  expect(queries.at(3)).toEqual({ text: "a\\.b\\(c\\)", case: "matched", words: "whole", syntax: "regex" });
  expect(queries.at(-1)).toEqual({ text: "a.b", case: "matched", words: "whole", syntax: "literal" });
  expect([search.matchCase, search.wholeWord, search.regex]).toEqual([true, true, false]);
});

it("asks nothing for an empty box, and waits for the diff before it asks, then asks again on a newer one", async () => {
  const search = new TextSearchState("python/cpython");
  core.textSearch.mockResolvedValue(running(1));
  core.textSearchState.mockResolvedValue(results(1, []));
  search.seed("");
  const empty = search.summary;
  search.seed("spam");
  await settled();
  const waiting = [search.summary, core.textSearch.mock.calls.length];

  search.follow("abc");
  await settled();
  search.follow("abc");
  search.follow("def");
  await settled();

  expect(empty).toBe("");
  expect(waiting).toEqual(["Waiting for the checkout…", 0]);
  expect(core.textSearch).toHaveBeenCalledTimes(2);
  expect(search.summary).toBe("No results");
});

it("says why a query can't run, and why the core couldn't be asked", async () => {
  const search = ready();
  core.textSearch.mockResolvedValueOnce({
    status: "ok",
    data: { kind: "bad_query", message: "unclosed group" },
  });
  search.seed("(");
  await settled();
  const bad = [search.summary, search.troubled];
  core.textSearch.mockResolvedValueOnce({ status: "error", error: { message: "not a Repository" } });
  search.seed("spam");
  await settled();
  const refused = search.summary;
  core.textSearch.mockRejectedValueOnce(new Error("no IPC"));
  search.seed("eggs");
  await settled();

  expect(bad).toEqual(["unclosed group", true]);
  expect(refused).toBe("Tsugiki can't search: not a Repository");
  expect(search.summary).toBe("Tsugiki can't search: Error: no IPC");
});

it("says why it can't hear the core, and listens again on the next query", async () => {
  core.listen.mockRejectedValueOnce(new Error("no events"));
  const search = ready(results(1, [fileHits("a.py", hit(1))]));
  search.seed("spam");
  await settled();
  const unheard = [search.summary, core.textSearch.mock.calls.length];

  search.seed("spam");
  await settled();

  expect(unheard).toEqual(["Tsugiki can't search: Error: no events", 0]);
  expect(search.summary).toBe("1 result in 1 file");
});

it("says what the core says of each state of the query, and the limit", async () => {
  const search = ready();
  search.seed("spam");
  await settled();
  const said = async (state: SearchState) => {
    core.textSearchState.mockResolvedValueOnce(state);
    core.changed();
    await settled();
    return search.summary;
  };

  const words = [
    await said({ state: "waiting", id: 1 }),
    await said(results(1, [], "running")),
    await said(results(1, [fileHits("a.py", hit(1))], "running")),
    await said({ state: "no_checkout", id: 1 }),
    await said({ state: "failed", id: 1 }),
    await said({ ...results(1, [fileHits("a.py", hit(1))], "stopped"), matches: 5000 }),
  ];

  expect(words).toEqual([
    "Waiting for the checkout…",
    "Searching…",
    "1 result in 1 file",
    "There's no checkout of the PR to search. tsugiki.log may say why.",
    "Tsugiki couldn't read the PR's files. tsugiki.log says why.",
    "Stopped at 5,000 results",
  ]);
});

it("shows only what answers the query asked last, and drops a late answer to an older one", async () => {
  const search = ready(results(2, [fileHits("b.py", hit(2))]));
  const first = Promise.withResolvers<unknown>();
  core.textSearch.mockReturnValueOnce(first.promise).mockResolvedValueOnce(running(2));
  search.seed("spam");
  await settled();
  search.seed("eggs");
  await settled();
  first.resolve(running(1));
  await settled();
  const shown = search.summary;
  // The core's state of an older query, or of none, changes nothing.
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("a.py", hit(1), hit(2))]));
  core.changed();
  await settled();
  core.textSearchState.mockResolvedValueOnce({ state: "idle" });
  core.changed();
  await settled();

  expect(shown).toBe("1 result in 1 file");
  expect(search.rows.map((row) => row.file.path)).toEqual(["b.py", "b.py"]);
});

it("reads once more after a read that changes came during, and says why a read failed", async () => {
  const search = ready();
  search.seed("spam");
  await settled();
  core.textSearchState.mockReset();
  const reading = Promise.withResolvers<SearchState>();
  core.textSearchState.mockReturnValueOnce(reading.promise);
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("a.py", hit(1))]));

  core.changed();
  core.changed();
  core.changed();
  reading.resolve(results(1, [], "running"));
  await settled();
  const read = [core.textSearchState.mock.calls.length, search.summary];
  core.textSearchState.mockRejectedValueOnce(new Error("gone"));
  core.changed();
  await settled();

  expect(read).toEqual([2, "1 result in 1 file"]);
  expect(search.summary).toBe("Tsugiki can't search: Error: gone");
});

it("reads nothing before the core has numbered a query", async () => {
  const search = ready();
  search.seed("spam");
  await settled();
  search.seed("");

  core.changed();
  await settled();

  expect(core.textSearchState).toHaveBeenCalledOnce();
});

it("moves the cursor through the rows, folds and unfolds a file, and picks a match", async () => {
  const search = ready(results(1, [fileHits("a.py", hit(1), hit(4)), fileHits("b.py", hit(2))]));
  search.seed("spam");
  await settled();
  const at = () => search.cursor;

  search.moveTo(-3);
  const top = at();
  search.unfold();
  const intoFile = at();
  search.moveTo(2);
  const picked = search.pick();
  search.fold();
  const toFile = at();
  search.fold();
  const folded = search.rows.length;
  search.fold();
  search.unfold();
  const unfolded = search.rows.length;
  search.moveTo(99);
  const last = at();
  search.unfold();
  search.moveTo(3);
  const onFile = search.pick();
  const flipped = search.rows.length;
  search.toggleFile("b.py");

  expect([top, intoFile, picked, toFile, folded, unfolded, last]).toEqual([
    0,
    1,
    { path: "a.py", line: 4 },
    0,
    3,
    5,
    4,
  ]);
  expect([onFile, flipped, search.rows.length]).toEqual([undefined, 4, 5]);
});

it("keeps the cursor on its row as files come in above it, and goes back to the top once the row goes", async () => {
  const search = ready(results(1, [fileHits("b.py", hit(2), hit(3))], "running"));
  search.seed("spam");
  await settled();
  search.moveTo(2);

  core.textSearchState.mockResolvedValueOnce(
    results(1, [fileHits("a.py", hit(1)), fileHits("b.py", hit(2), hit(3))]),
  );
  core.changed();
  await settled();
  const kept = search.cursor;
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("c.py", hit(1))]));
  core.changed();
  await settled();

  expect([kept, search.cursor]).toEqual([4, 0]);
  expect(search.pick()).toBeUndefined();
});

it("picks nothing and moves nowhere with no rows", () => {
  const search = new TextSearchState("python/cpython");

  search.moveTo(1);
  search.fold();
  search.unfold();

  expect([search.rows, search.cursor, search.pick()]).toEqual([[], 0, undefined]);
});

it("stops listening as the PR view goes, even while the listener is on its way", async () => {
  const placed = Promise.withResolvers<() => void>();
  core.listen.mockReturnValueOnce(placed.promise);
  const search = ready();
  search.seed("spam");
  await settled();

  search.stop();
  placed.resolve(core.stop);
  await settled();

  expect(core.stop).toHaveBeenCalledOnce();
  expect(core.textSearch).not.toHaveBeenCalled();
});

it("stops a query that would run after a pause, and writes text as a regex that matches it", async () => {
  vi.useFakeTimers();
  const search = ready();
  search.type("spam");
  search.stop();
  await vi.advanceTimersByTimeAsync(SEARCH_PAUSE);

  expect(core.textSearch).not.toHaveBeenCalled();
  expect(literalRegex("a+b*c?^$.|[x]{2}\\")).toBe("a\\+b\\*c\\?\\^\\$\\.\\|\\[x\\]\\{2\\}\\\\");
});
