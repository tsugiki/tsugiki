import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { FileHits, LineHit, SearchState } from "$lib/data/bindings";
import { nextTask } from "$lib/testing/helpers";
import {
  literalRegex,
  SEARCH_PAUSE,
  type SearchDiff,
  searchDiff,
  TextSearchState,
} from "./text-search.svelte";

const core = vi.hoisted(() => ({
  textSearch: vi.fn(),
  textSearchState: vi.fn(),
  textSearchCancel: vi.fn(),
  listen: vi.fn(),
  /** Tells the window that what the core found may have changed. */
  changed: () => {},
  stop: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    textSearch: core.textSearch,
    textSearchState: core.textSearchState,
    textSearchCancel: core.textSearchCancel,
  },
  events: { textSearchChanged: { listen: core.listen } },
}));

/** The diff the search runs on, from the merge base `b0` to the head `abc`. */
const READY: SearchDiff = { kind: "ready", base: "b0", head: "abc" };

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
/** What each query asked the core: its diff's ends and its options. */
const asked = () =>
  core.textSearch.mock.calls.map((call: unknown[]) => {
    const { base, head, query } = call[0] as { base: string; head: string; query: unknown };
    return { base, head, query };
  });
const cancelled = () => core.textSearchCancel.mock.calls.map((call: unknown[]) => call[0]);

/** A search on the diff `READY`, whose core answers each query with the next number and `state`. */
const ready = (state: SearchState = { state: "idle" }) => {
  let id = 0;
  core.textSearch.mockImplementation(() => {
    id += 1;
    return Promise.resolve(running(id));
  });
  core.textSearchState.mockResolvedValue(state);
  const search = new TextSearchState("python/cpython");
  search.follow(READY);
  return search;
};

beforeEach(() => {
  vi.resetAllMocks();
  core.listen.mockImplementation((heard: () => void) => {
    core.changed = heard;
    return Promise.resolve(core.stop);
  });
  core.textSearchCancel.mockResolvedValue(null);
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
    [
      {
        repository: "python/cpython",
        base: "b0",
        head: "abc",
        query: { text: "spam", case: "ignored", words: "within", syntax: "literal" },
      },
    ],
  ]);
  expect(search.summary).toBe("4 results in 2 files");
  expect([search.troubled, search.answered]).toEqual([false, 1]);
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
  await nextTask();
  search.seed("a.b(c)");
  await nextTask();
  search.toggle("regex");
  search.seed("a.b");
  await nextTask();

  // Each query goes in place of those asked before it while the listener was on its way.
  expect(asked().map((each) => each.query)).toEqual([
    { text: "spam", case: "matched", words: "whole", syntax: "regex" },
    { text: "a\\.b\\(c\\)", case: "matched", words: "whole", syntax: "regex" },
    { text: "a.b", case: "matched", words: "whole", syntax: "literal" },
  ]);
  expect([search.matchCase, search.wholeWord, search.regex]).toEqual([true, true, false]);
});

it("asks nothing for an empty box, waits for the diff, then asks again on a diff with another end", async () => {
  const search = new TextSearchState("python/cpython");
  core.textSearch.mockResolvedValue(running(1));
  core.textSearchState.mockResolvedValue(results(1, []));
  search.seed("");
  const empty = search.summary;
  search.seed("spam");
  await nextTask();
  const waiting = [search.summary, core.textSearch.mock.calls.length];

  search.follow(READY);
  await nextTask();
  search.follow({ ...READY });
  // A PR moved to another Target branch keeps its head and gets another merge base (#211).
  search.follow({ ...READY, base: "b1" });
  await nextTask();
  search.follow({ ...READY, base: "b1", head: "def" });
  await nextTask();

  expect(empty).toBe("");
  expect(waiting).toEqual(["Waiting for the checkout…", 0]);
  expect(asked().map(({ base, head }) => [base, head])).toEqual([
    ["b0", "abc"],
    ["b1", "abc"],
    ["b1", "def"],
  ]);
  expect(search.summary).toBe("No results");
});

it("says why it waits for a diff that won't come on its own, and cancels the query once its diff goes", async () => {
  const search = ready(results(1, [fileHits("a.py", hit(1))], "running"));
  search.seed("spam");
  await nextTask();
  const said = (diff: SearchDiff) => {
    search.follow(diff);
    return [search.summary, search.troubled];
  };

  const words = [
    said({ kind: "loading" }),
    said({ kind: "offline" }),
    said({ kind: "asking" }),
    said({ kind: "absent" }),
    said({ kind: "failed" }),
  ];

  expect(words).toEqual([
    ["Waiting for the checkout…", false],
    ["The search waits for the PR's commits, which come once GitHub answers.", false],
    ["The search waits for the Repository's clone, which waits for an answer in the Files tab.", false],
    ["Tsugiki doesn't hold this PR, so there's nothing to search.", true],
    ["The PR's diff didn't load, so there's nothing to search. The Files tab says why.", true],
  ]);
  expect(search.rows).toEqual([]);
  expect(cancelled()).toEqual([1]);
});

it("gives the search the Files tab's diff once it is ready, or why there is none", () => {
  type Files = Parameters<typeof searchDiff>[0];
  const ready = { state: "ready", base: "b0", head: "abc" } as Files["view"];
  const fetching: Files["view"] = { state: "fetching" };
  const files = (view: Files["view"], more: Partial<Files> = {}) =>
    searchDiff({ view, error: undefined, unfetched: undefined, waitsForNetwork: false, ...more });

  expect([
    files(ready, { error: "a chunk didn't come" }),
    files(undefined),
    files(fetching),
    files({ state: "asking" }),
    files(null),
    files(undefined, { error: "no network" }),
    // The mirror stopped fetching the PR's head without it, so the diff won't come on its own, unless the
    // network explains it, and the Files tab tries again by itself.
    files(fetching, { unfetched: { kind: "not_on_github" } }),
    files(fetching, { unfetched: { kind: "git", words: "timed out" }, waitsForNetwork: true }),
    // Only the PR's commits wait for the network here: a clone that does says so as `loading`.
    files({ state: "cloning" }, { waitsForNetwork: true }),
  ]).toEqual([
    READY,
    { kind: "loading" },
    { kind: "loading" },
    { kind: "asking" },
    { kind: "absent" },
    { kind: "failed" },
    { kind: "failed" },
    { kind: "offline" },
    { kind: "loading" },
  ]);
});

it("says why a query can't run, and why the core couldn't be asked", async () => {
  const search = ready();
  core.textSearch.mockResolvedValueOnce({
    status: "ok",
    data: { kind: "bad_query", message: "unclosed group" },
  });
  search.seed("(");
  await nextTask();
  const bad = [search.summary, search.troubled];
  core.textSearch.mockResolvedValueOnce({ status: "error", error: { message: "not a Repository" } });
  search.seed("spam");
  await nextTask();
  const refused = search.summary;
  core.textSearch.mockRejectedValueOnce(new Error("no IPC"));
  search.seed("eggs");
  await nextTask();

  expect(bad).toEqual(["unclosed group", true]);
  expect(refused).toBe("Tsugiki can't search: not a Repository");
  expect(search.summary).toBe("Tsugiki can't search: Error: no IPC");
});

it("cancels the query an empty box, a bad query, a newer query's late answer or the view's end leaves running", async () => {
  const search = ready();
  search.seed("spam");
  await nextTask();
  search.seed("");
  await nextTask();
  search.seed("spam");
  await nextTask();
  core.textSearch.mockResolvedValueOnce({ status: "ok", data: { kind: "bad_query", message: "x" } });
  search.seed("(");
  await nextTask();
  const late = Promise.withResolvers<unknown>();
  core.textSearch.mockReturnValueOnce(late.promise);
  search.seed("eggs");
  await nextTask();
  search.seed("ham");
  await nextTask();
  late.resolve(running(7));
  await nextTask();
  search.stop();
  await nextTask();

  // The second query, 2, ran until the bad one; the late answer, 7, came after the next, 3.
  expect(cancelled()).toEqual([1, 2, 7, 3]);
});

it("cancels the query before one that a newer one, or the view's end, stops on its way to the core", async () => {
  const overtaken = ready();
  overtaken.seed("spam");
  await nextTask();
  overtaken.seed("eggs");
  overtaken.seed("");
  await nextTask();
  const byNewer = cancelled();
  core.textSearchCancel.mockClear();
  const stopped = ready();
  stopped.seed("spam");
  await nextTask();
  stopped.seed("eggs");
  stopped.stop();
  await nextTask();

  expect([byNewer, cancelled()]).toEqual([[1], [1]]);
  expect(core.textSearch).toHaveBeenCalledTimes(2);
});

it("says nothing of a cancel the core couldn't take", async () => {
  core.textSearchCancel.mockRejectedValue(new Error("no core"));
  const search = ready();
  search.seed("spam");
  await nextTask();

  search.seed("");
  await nextTask();

  expect(cancelled()).toEqual([1]);
  expect(search.troubled).toBe(false);
});

it("sends nothing of a query a newer one replaced while the listener was on its way", async () => {
  const placed = Promise.withResolvers<() => void>();
  core.listen.mockReturnValueOnce(placed.promise);
  const search = ready();

  search.seed("e");
  search.seed("");
  placed.resolve(core.stop);
  await nextTask();

  expect(core.textSearch).not.toHaveBeenCalled();
  expect(search.summary).toBe("");
});

it("says why it can't hear the core, and listens again on the next query", async () => {
  core.listen.mockRejectedValueOnce(new Error("no events"));
  const search = ready(results(1, [fileHits("a.py", hit(1))]));
  search.seed("spam");
  await nextTask();
  const unheard = [search.summary, core.textSearch.mock.calls.length];

  search.seed("spam");
  await nextTask();

  expect(unheard).toEqual(["Tsugiki can't search: Error: no events", 0]);
  expect(search.summary).toBe("1 result in 1 file");
});

it("says what the core says of each state of the query, and the limit", async () => {
  const search = ready();
  search.seed("spam");
  await nextTask();
  const said = async (state: SearchState) => {
    core.textSearchState.mockResolvedValueOnce(state);
    core.changed();
    await nextTask();
    return [search.summary, search.troubled];
  };

  const words = [
    await said({ state: "waiting", id: 1 }),
    await said(results(1, [], "running")),
    await said(results(1, [fileHits("a.py", hit(1))], "running")),
    await said({ state: "failed", id: 1 }),
    await said({ ...results(1, [fileHits("a.py", hit(1))], "stopped"), matches: 5000 }),
    // The diff is read again, and the query runs once more: see the next test.
    await said({ state: "no_checkout", id: 1 }),
  ];

  expect(words).toEqual([
    ["Waiting for the checkout…", false],
    ["Searching…", false],
    ["1 result in 1 file", false],
    ["Tsugiki couldn't read the PR's files. tsugiki.log says why.", true],
    ["Stopped at 5,000 results", false],
    ["Waiting for the checkout…", false],
  ]);
});

it("reads the diff again once when the core finds no checkout of it, and runs the query once more", async () => {
  const reread = vi.fn(() => Promise.resolve());
  let id = 0;
  core.textSearch.mockImplementation(() => {
    id += 1;
    return Promise.resolve(running(id));
  });
  const search = new TextSearchState("python/cpython", reread);
  search.follow(READY);
  const noCheckout = async () => {
    core.textSearchState.mockResolvedValueOnce({ state: "no_checkout", id });
    core.changed();
    await nextTask();
    return [search.summary, search.troubled, reread.mock.calls.length, core.textSearch.mock.calls.length];
  };
  core.textSearchState.mockResolvedValue({ state: "waiting", id: 1 });
  search.seed("spam");
  await nextTask();

  const first = await noCheckout();
  const second = await noCheckout();
  // A new query reads the diff again once more, but not if the user typed meanwhile.
  const later = Promise.withResolvers<undefined>();
  reread.mockReturnValueOnce(later.promise);
  search.seed("eggs");
  await nextTask();
  const third = await noCheckout();
  search.seed("ham");
  await nextTask();
  later.resolve(undefined);
  await nextTask();

  expect(first).toEqual(["Waiting for the checkout…", false, 1, 2]);
  expect(second).toEqual(["There's no checkout of the PR to search. tsugiki.log may say why.", true, 1, 2]);
  expect(third).toEqual(["Waiting for the checkout…", false, 2, 3]);
  expect(core.textSearch).toHaveBeenCalledTimes(4);
});

it("knows the text the box holds, as a regex with regex on", () => {
  const search = new TextSearchState("python/cpython");
  search.text = "a.b";
  const literal = [search.holds("a.b"), search.holds("a")];
  search.regex = true;
  search.text = "a\\.b";

  expect([...literal, search.holds("a.b")]).toEqual([true, false, true]);
});

it("shows only what answers the query asked last, and drops a late answer to an older one", async () => {
  const search = ready(results(2, [fileHits("b.py", hit(2))]));
  const first = Promise.withResolvers<unknown>();
  core.textSearch.mockReturnValueOnce(first.promise).mockResolvedValueOnce(running(2));
  search.seed("spam");
  await nextTask();
  search.seed("eggs");
  await nextTask();
  first.resolve(running(1));
  await nextTask();
  const shown = search.summary;
  // The core's state of an older query, or of none, changes nothing.
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("a.py", hit(1), hit(2))]));
  core.changed();
  await nextTask();
  core.textSearchState.mockResolvedValueOnce({ state: "idle" });
  core.changed();
  await nextTask();

  expect(shown).toBe("1 result in 1 file");
  expect(search.rows.map((row) => row.file.path)).toEqual(["b.py", "b.py"]);
});

it("reads once more after a read that changes came during, and says why a read failed", async () => {
  const search = ready();
  search.seed("spam");
  await nextTask();
  core.textSearchState.mockReset();
  const reading = Promise.withResolvers<SearchState>();
  core.textSearchState.mockReturnValueOnce(reading.promise);
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("a.py", hit(1))]));

  core.changed();
  core.changed();
  core.changed();
  reading.resolve(results(1, [], "running"));
  await nextTask();
  const read = [core.textSearchState.mock.calls.length, search.summary];
  core.textSearchState.mockRejectedValueOnce(new Error("gone"));
  core.changed();
  await nextTask();

  expect(read).toEqual([2, "1 result in 1 file"]);
  expect(search.summary).toBe("Tsugiki can't search: Error: gone");
});

it("reads nothing before the core has numbered a query", async () => {
  const search = ready();
  search.seed("spam");
  await nextTask();
  search.seed("");

  core.changed();
  await nextTask();

  expect(core.textSearchState).toHaveBeenCalledOnce();
});

it("moves the cursor through the rows, folds and unfolds a file, and picks a match at the diff's ends", async () => {
  const search = ready(results(1, [fileHits("a.py", hit(1), hit(4)), fileHits("b.py", hit(2))]));
  search.seed("spam");
  await nextTask();
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
    { path: "a.py", line: 4, ends: { base: "b0", head: "abc" } },
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
  await nextTask();
  search.moveTo(2);

  core.textSearchState.mockResolvedValueOnce(
    results(1, [fileHits("a.py", hit(1)), fileHits("b.py", hit(2), hit(3))]),
  );
  core.changed();
  await nextTask();
  const kept = search.cursor;
  core.textSearchState.mockResolvedValueOnce(results(1, [fileHits("c.py", hit(1))]));
  core.changed();
  await nextTask();

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
  await nextTask();

  search.stop();
  placed.resolve(core.stop);
  await nextTask();

  expect(core.stop).toHaveBeenCalledOnce();
  expect(core.textSearch).not.toHaveBeenCalled();
});

it("places no listener and asks nothing once the PR view has gone", async () => {
  const search = ready();

  search.stop();
  search.seed("spam");
  await nextTask();

  expect(core.listen).not.toHaveBeenCalled();
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
