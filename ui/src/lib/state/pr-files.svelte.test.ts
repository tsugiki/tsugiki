import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Answer, DefinitionLookup, DiffLine, Hit, Waiting } from "$lib/data/bindings";
import {
  CHUNK,
  DiffViewChoice,
  EXPAND,
  NOTICE_MS,
  PrFilesState,
  SPANS_WAIT,
  type DiffFile,
  type DiffRow,
  type DiffThread,
  type PrFiles,
} from "./pr-files.svelte";
import { PEEK_ID, PEEK_ROWS } from "./peek";
import type { YourReview } from "./workbench.svelte";

type Heard = (event: { payload: unknown }) => void;

/** The channel a lookup hears what it waits for on, which a test speaks into as the core would. */
interface FakeChannel {
  onmessage: (message: Waiting) => void;
}

vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: (message: Waiting) => void = () => {};
  },
}));

const core = vi.hoisted(() => {
  const core = {
    pullRequestFiles: vi.fn(),
    pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
    pullRequestSpans: vi.fn<(chunk: { start: number }, opened?: number[]) => Promise<unknown>>(),
    pullRequestLines: vi.fn<(lines: { file: number; start: number; end: number }) => Promise<unknown>>(),
    pullRequestFind: vi.fn<(search: { query: string }) => Promise<unknown>>(),
    definitions: vi.fn<(lookup: DefinitionLookup, waiting: FakeChannel) => Promise<unknown>>(),
    cancelDefinitions: vi.fn(() => Promise.resolve()),
    pullRequestRow: vi.fn<(line: DiffLine) => Promise<unknown>>(),
    cloning: vi.fn(),
    mirrorStates: vi.fn(),
    mirrorsHeld: vi.fn(),
    heard: {} as Record<string, Heard>,
    stopped: [] as string[],
    /** An event whose listener lands in `heard`, and whose stop lands in `stopped`. */
    listener: (name: string) => ({
      listen: (heard: Heard) => {
        core.heard[name] = heard;
        return Promise.resolve(() => core.stopped.push(name));
      },
    }),
  };
  return core;
});

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequestFiles: core.pullRequestFiles,
    pullRequestRows: core.pullRequestRows,
    pullRequestSpans: core.pullRequestSpans,
    pullRequestLines: core.pullRequestLines,
    pullRequestFind: core.pullRequestFind,
    definitions: core.definitions,
    cancelDefinitions: core.cancelDefinitions,
    pullRequestRow: core.pullRequestRow,
    cloning: core.cloning,
    mirrorStates: core.mirrorStates,
    mirrorsHeld: core.mirrorsHeld,
  },
  events: {
    headsFetched: core.listener("headsFetched"),
    headsFetchFailed: core.listener("headsFetchFailed"),
    changed: core.listener("changed"),
    cloningChanged: core.listener("cloningChanged"),
    mirrorStatesChanged: core.listener("mirrorStatesChanged"),
    mirrorsHeldChanged: core.listener("mirrorsHeldChanged"),
  },
}));

/** A file of the diff whose header is row `row`. */
const file = (path: string, row: number, generated = false): DiffFile => ({
  path,
  old_path: null,
  change: { kind: "lines", added: 1, removed: 0 },
  row,
  columns: 4,
  generated,
  gaps: [],
  rest: null,
  pairs: [],
});

/** A diff from `base` to `head`. */
const ready = (
  head: string,
  rows: number,
  files = [file("NEWS", 0)],
  base = "base",
): Extract<PrFiles, { state: "ready" }> => ({
  state: "ready",
  head,
  base,
  files,
  rows,
  news: [],
  threads: { kind: "shown", threads: [] },
});

/** A file, a Generated one from row 10 to the third chunk, and a file in the third chunk. */
const withGenerated = (head: string) =>
  ready(head, 3 * CHUNK, [
    file("Lib/ast.py", 0),
    file("Python/generated_cases.c.h", 10, true),
    file("Lib/os.py", 2 * CHUNK),
  ]);

/** Where each chunk asked for starts. */
const starts = () => core.pullRequestRows.mock.calls.map(([chunk]) => chunk.start);

const ok = <T>(data: T) => ({ status: "ok" as const, data });
const added = (text: string): DiffRow => ({ kind: "added", new: 1, text });
/** The spans the fake core sends for the first row of the chunk at `start`. */
const keyword = (start: number) => [{ start: 0, end: start + 1, highlight: "keyword" as const }];
const cloning = { repository: "python/cpython", progress: { stage: "receiving", percent: 40 } } as const;

/** A HUD that notes what each jump it times says, numbered from 1. */
const timer = () => {
  const seen: string[] = [];
  const jumped = vi.fn(() => {
    const jump = String(jumped.mock.calls.length);
    return {
      rowsShown: (highlighted: boolean) =>
        seen.push(`${jump}: rows ${highlighted ? "highlighted" : "plain"}`),
      spansShown: () => seen.push(`${jump}: spans`),
    };
  });
  return { jumped, seen };
};

/** A Files tab for PR 11 that has loaded, with the core answering `answer`. */
const loaded = async (answer: PrFiles | null = { state: "fetching" }, jumps = timer()) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, jumps);
  await files.load();
  return files;
};

/** Rows that come only when the test answers them. */
const heldRows = () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestRows.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  return (rows: unknown) => {
    answer(rows);
  };
};

/** Waits until what the answers so far set going has run its course. */
const settled = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Spans that come only when the test answers them. */
const heldSpans = () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestSpans.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  return (spans: unknown) => {
    answer(spans);
  };
};

beforeEach(() => {
  vi.resetAllMocks();
  core.heard = {};
  core.stopped = [];
  core.cloning.mockResolvedValue(null);
  core.mirrorStates.mockResolvedValue([]);
  core.mirrorsHeld.mockResolvedValue(false);
  core.pullRequestRows.mockImplementation(({ start }: { start: number }) =>
    Promise.resolve(ok([added(`row ${String(start)}`)])),
  );
  core.pullRequestSpans.mockImplementation(({ start }: { start: number }) =>
    Promise.resolve(ok([keyword(start)])),
  );
  core.pullRequestLines.mockImplementation(({ start, end }) => Promise.resolve(ok(headLines(start, end))));
});

afterEach(() => {
  vi.useRealTimers();
});

it("shows a chunk's rows with their spans when the spans come in time, and times one jump", async () => {
  const jumps = timer();
  const files = await loaded(ready("abc", 3 * CHUNK), jumps);

  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.row(CHUNK)).toBeDefined();
  });

  expect(files.row(0)).toEqual(added("row 0"));
  expect(files.spans(0)).toEqual(keyword(0));
  expect(files.spans(1)).toBeUndefined();
  expect(core.pullRequestSpans).toHaveBeenCalledWith(
    { repository: "python/cpython", number: 11, head: "abc", base: "base", start: 0, end: CHUNK },
    [],
  );
  // The chunk after only comes near, so it isn't a jump.
  expect(jumps.seen).toEqual(["1: rows highlighted"]);
  files.need(5, 20);
  files.need(2 * CHUNK, 2 * CHUNK + 9);
  expect(jumps.jumped).toHaveBeenCalledTimes(2);
});

it("waits 50 ms for a chunk's spans, then shows its rows plain and the spans when they come", async () => {
  const jumps = timer();
  const files = await loaded(ready("abc", 2 * CHUNK), jumps);
  vi.useFakeTimers();
  const [first, after] = [heldSpans(), heldSpans()];

  files.need(0, 9);
  await vi.advanceTimersByTimeAsync(SPANS_WAIT - 1);
  expect(files.row(0)).toBeUndefined();
  await vi.advanceTimersByTimeAsync(1);
  expect(files.row(0)).toEqual(added("row 0"));
  expect(files.spans(0)).toBeUndefined();
  first(ok([keyword(0)]));
  await vi.advanceTimersByTimeAsync(0);
  expect(files.spans(0)).toEqual(keyword(0));

  // The chunk after waits for those that show, and then for its own spans.
  expect(core.pullRequestRows).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(SPANS_WAIT);
  expect(files.row(CHUNK)).toEqual(added(`row ${String(CHUNK)}`));
  after(ok([keyword(CHUNK)]));
  await vi.advanceTimersByTimeAsync(0);
  expect(files.spans(CHUNK)).toEqual(keyword(CHUNK));
  expect(jumps.seen).toEqual(["1: rows plain", "1: spans"]);
});

it("asks for the chunk after those that show once their spans have come", async () => {
  const files = await loaded(ready("abc", 2 * CHUNK));
  const answer = heldSpans();

  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.row(0)).toBeDefined();
  });
  expect(core.pullRequestRows).toHaveBeenCalledOnce();
  answer(ok([keyword(0)]));

  await vi.waitFor(() => {
    expect(files.row(CHUNK)).toBeDefined();
  });
});

it("lets go of a chunk's spans with its rows, and drops those that come once it's gone", async () => {
  const files = await loaded(ready("abc", 20 * CHUNK));
  files.need(0, 10);
  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(0));
  });
  const answerLate = heldSpans();
  files.need(5 * CHUNK, 5 * CHUNK + 10);
  await vi.waitFor(() => {
    expect(files.row(5 * CHUNK)).toBeDefined();
  });
  files.need(10 * CHUNK, 10 * CHUNK + 10);
  answerLate(ok([keyword(5 * CHUNK)]));

  await vi.waitFor(() => {
    expect(files.spans(10 * CHUNK)).toEqual(keyword(10 * CHUNK));
  });
  expect(files.spans(0)).toBeUndefined();
  expect(files.spans(5 * CHUNK)).toBeUndefined();
  core.pullRequestFiles.mockResolvedValue(ok(ready("new", 10)));
  await files.ask();
  expect(files.spans(10 * CHUNK)).toBeUndefined();
});

it("shows a chunk whose spans don't come plain, and leaves its rows to say what went wrong", async () => {
  const files = await loaded(ready("abc", 2 * CHUNK));
  core.pullRequestSpans.mockResolvedValueOnce({
    status: "error",
    error: { message: '"abc" isn\'t a commit.' },
  });
  core.pullRequestSpans.mockRejectedValueOnce(new Error("gone"));

  files.need(0, 9);

  await vi.waitFor(() => {
    expect(files.row(CHUNK)).toEqual(added(`row ${String(CHUNK)}`));
  });
  expect(files.spans(0)).toBeUndefined();
  expect(files.spans(CHUNK)).toBeUndefined();
  expect(files.error).toBeUndefined();
});

it("asks the core for the PR's files and the clone under way", async () => {
  core.cloning.mockResolvedValue(cloning);

  const files = await loaded();

  expect(core.pullRequestFiles).toHaveBeenCalledExactlyOnceWith("python/cpython", 11);
  expect(files.view).toEqual({ state: "fetching" });
  expect(files.cloning).toEqual(cloning);
  expect(files.error).toBeUndefined();
});

it("holds null for a PR Tsugiki doesn't hold", async () => {
  const files = await loaded(null);

  expect(files.view).toBeNull();
});

it("asks again when the PR's head arrives or a sync changes the PR, and not for other PRs", async () => {
  const files = await loaded();
  core.pullRequestFiles.mockResolvedValue(ok(ready("abc", 3)));

  core.heard.headsFetched?.({ payload: { repository: "python/cpython", items: [12] } });
  core.heard.changed?.({ payload: { repository: "python/peps", items: [11] } });
  await vi.waitFor(() => {
    expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
  });
  core.heard.headsFetched?.({ payload: { repository: "python/cpython", items: [11, 12] } });
  await vi.waitFor(() => {
    expect(files.view).toEqual(ready("abc", 3));
  });
  core.heard.changed?.({ payload: { repository: "python/cpython", items: [11] } });

  await vi.waitFor(() => {
    expect(core.pullRequestFiles).toHaveBeenCalledTimes(3);
  });
});

it("follows the clone, and asks again once it ends", async () => {
  const files = await loaded({ state: "cloning" });

  core.heard.cloningChanged?.({ payload: cloning });
  expect(files.cloning).toEqual(cloning);
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
  core.pullRequestFiles.mockResolvedValue(ok({ state: "fetching" }));
  core.heard.cloningChanged?.({ payload: null });

  expect(files.cloning).toBeNull();
  await vi.waitFor(() => {
    expect(files.view).toEqual({ state: "fetching" });
  });
});

/** The mirror states the core tells of, with python/cpython's in `state` beside another Repository's. */
const states = (state: "asking" | "waiting") => ({
  payload: [
    { repository: "octo-org/hello", state: { state: "asking" } },
    { repository: "python/cpython", state: state === "asking" ? { state } : { state, behind: null } },
  ],
});

it("asks again once the user answers where the mirror gets its objects", async () => {
  const files = await loaded({ state: "asking" });
  expect(files.repository).toBe("python/cpython");

  core.heard.mirrorStatesChanged?.(states("asking"));
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
  core.pullRequestFiles.mockResolvedValue(ok({ state: "cloning" }));
  core.heard.mirrorStatesChanged?.(states("waiting"));

  await vi.waitFor(() => {
    expect(files.view).toEqual({ state: "cloning" });
  });
});

it("asks again when the clone it waits for comes to wait for an answer", async () => {
  const files = await loaded({ state: "cloning" });

  core.heard.mirrorStatesChanged?.(states("waiting"));
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
  core.pullRequestFiles.mockResolvedValue(ok({ state: "asking" }));
  core.heard.mirrorStatesChanged?.(states("asking"));

  await vi.waitFor(() => {
    expect(files.view).toEqual({ state: "asking" });
  });
});

it("asks again when the clone comes to wait for an answer while the core answers", async () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFiles.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  core.pullRequestFiles.mockResolvedValue(ok({ state: "asking" }));
  const files = new PrFilesState("python/cpython", 11);
  const loading = files.load();
  await vi.waitFor(() => {
    expect(core.pullRequestFiles).toHaveBeenCalledOnce();
  });

  core.heard.mirrorStatesChanged?.(states("asking"));
  answer(ok({ state: "cloning" }));
  await loading;

  await vi.waitFor(() => {
    expect(files.view).toEqual({ state: "asking" });
  });
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(2);
});

it("finds its Repository's mirror by its name in any case", async () => {
  const files = await loaded({ state: "asking" });

  const told = states("asking");
  told.payload[1] = { repository: "Python/CPython", state: { state: "asking" } };
  core.heard.mirrorStatesChanged?.(told);

  expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
  expect(files.view).toEqual({ state: "asking" });
});

it("keeps its Repository's mirror state, as the core answers and then tells of it", async () => {
  core.mirrorStates.mockResolvedValue([{ repository: "Python/CPython", state: { state: "offline" } }]);
  const files = await loaded({ state: "cloning" });
  expect(files.mirror).toEqual({ state: "offline" });

  core.heard.mirrorStatesChanged?.({
    payload: [{ repository: "python/cpython", state: { state: "cloning", progress: { stage: "starting" } } }],
  });

  expect(files.mirror).toEqual({ state: "cloning", progress: { stage: "starting" } });
  expect(core.pullRequestFiles).toHaveBeenCalledOnce();
});

it("keeps mirror states and a hold it heard of while asking over the older answers", async () => {
  core.pullRequestFiles.mockResolvedValue(ok({ state: "cloning" }));
  core.mirrorStates.mockImplementation(() => {
    core.heard.mirrorStatesChanged?.(states("waiting"));
    return Promise.resolve([{ repository: "python/cpython", state: { state: "offline" } }]);
  });
  core.mirrorsHeld.mockImplementation(() => {
    core.heard.mirrorsHeldChanged?.({ payload: true });
    return Promise.resolve(false);
  });
  const files = new PrFilesState("python/cpython", 11);

  await files.load();

  expect(files.mirror).toEqual({ state: "waiting", behind: null });
  expect(files.held).toBe(true);
});

it("hears that the worker holds back git, and asks again once it goes on", async () => {
  core.mirrorsHeld.mockResolvedValue(true);
  const files = await loaded({ state: "fetching" });
  expect(files.held).toBe(true);
  core.heard.mirrorsHeldChanged?.({ payload: true });
  expect(core.pullRequestFiles).toHaveBeenCalledOnce();

  core.pullRequestFiles.mockResolvedValue(ok(ready("abc", 3)));
  core.heard.mirrorsHeldChanged?.({ payload: false });

  expect(files.held).toBe(false);
  await vi.waitFor(() => {
    expect(files.view).toEqual(ready("abc", 3));
  });
});

it("back online, asks again for a newer head it waits for, and not for a diff it has", async () => {
  const before = { ...ready("older", 3), threads: { kind: "held_back" as const, newer: "newer", count: 0 } };
  const files = await loaded(before);
  core.heard.mirrorsHeldChanged?.({ payload: true });
  core.pullRequestFiles.mockResolvedValue(ok(ready("newer", 3)));
  core.heard.mirrorsHeldChanged?.({ payload: false });
  await vi.waitFor(() => {
    expect(files.view).toEqual(ready("newer", 3));
  });

  core.heard.mirrorsHeldChanged?.({ payload: true });
  core.heard.mirrorsHeldChanged?.({ payload: false });

  expect(core.pullRequestFiles).toHaveBeenCalledTimes(2);
});

it("keeps a clone it heard of while asking over the older answer", async () => {
  core.pullRequestFiles.mockResolvedValue(ok({ state: "cloning" }));
  core.cloning.mockImplementation(() => {
    core.heard.cloningChanged?.({ payload: cloning });
    return Promise.resolve(null);
  });
  const files = new PrFilesState("python/cpython", 11);

  await files.load();

  expect(files.cloning).toEqual(cloning);
});

it("stops listening, even when it stops before it started", async () => {
  const files = await loaded();
  files.stop();
  expect(core.stopped.toSorted()).toEqual([
    "changed",
    "cloningChanged",
    "headsFetchFailed",
    "headsFetched",
    "mirrorStatesChanged",
    "mirrorsHeldChanged",
  ]);

  core.stopped = [];
  const early = new PrFilesState("python/cpython", 11);
  const loading = early.load();
  early.stop();
  await loading;

  expect(core.stopped).toHaveLength(6);
  expect(core.cloning).toHaveBeenCalledTimes(1);
});

it("says why it can't listen", async () => {
  core.pullRequestFiles.mockResolvedValue(ok(null));
  core.cloning.mockRejectedValue(new Error("no core"));
  const files = new PrFilesState("python/cpython", 11);

  await files.load();

  expect(files.error).toBe("Error: no core");
});

it("says why the core couldn't answer, and forgets it once the core answers", async () => {
  core.pullRequestFiles.mockResolvedValue({ status: "error", error: { message: "The mirror has no main" } });
  const files = new PrFilesState("python/cpython", 11);
  await files.load();
  expect(files.error).toBe("The mirror has no main");
  expect(files.view).toBeUndefined();

  core.pullRequestFiles.mockRejectedValue("gone");
  await files.ask();
  expect(files.error).toBe("gone");

  core.pullRequestFiles.mockResolvedValue(ok({ state: "fetching" }));
  await files.ask();
  expect(files.error).toBeUndefined();
});

it("drops an older answer that arrives after a newer one", async () => {
  const files = await loaded();
  let answerOlder: (answer: unknown) => void = () => {};
  core.pullRequestFiles.mockReturnValueOnce(
    new Promise((resolve) => {
      answerOlder = resolve;
    }),
  );
  core.pullRequestFiles.mockResolvedValueOnce(ok(ready("new", 3)));

  const older = files.ask();
  await files.ask();
  answerOlder(ok(ready("old", 3)));
  await older;

  expect(files.view).toEqual(ready("new", 3));
});

it("asks for the chunks near what shows, and the one after, once each", async () => {
  const files = await loaded(ready("abc", 3 * CHUNK + 10));

  files.need(0, 10);
  files.need(5, 20);
  await vi.waitFor(() => {
    expect(files.row(CHUNK)).toEqual(added(`row ${String(CHUNK)}`));
  });

  expect(core.pullRequestRows.mock.calls).toEqual([
    [{ repository: "python/cpython", number: 11, head: "abc", base: "base", start: 0, end: CHUNK }],
    [{ repository: "python/cpython", number: 11, head: "abc", base: "base", start: CHUNK, end: 2 * CHUNK }],
  ]);
  expect(files.row(0)).toEqual(added("row 0"));
  expect(files.row(1)).toBeUndefined();
  expect(files.row(2 * CHUNK)).toBeUndefined();
});

it("asks for no chunk past the last", async () => {
  const files = await loaded(ready("abc", CHUNK + 1));

  files.need(CHUNK - 5, CHUNK);

  expect(core.pullRequestRows.mock.calls.map(([chunk]) => chunk.start)).toEqual([0, CHUNK]);
});

it("asks for no rows until the diff is ready", async () => {
  const files = await loaded();

  files.need(0, 10);

  expect(core.pullRequestRows).not.toHaveBeenCalled();
  expect(files.layout.count).toBe(0);
  expect(files.collapsed(0)).toBe(false);
  files.toggle(0);
  expect(core.pullRequestSpans).not.toHaveBeenCalled();
});

it("starts a Generated file collapsed, asks for none of its rows, and opens it on a toggle", async () => {
  const files = await loaded(withGenerated("abc"));

  const collapsed = [0, 1, 2].map((at) => files.collapsed(at));
  const shown = files.layout.count;
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(core.pullRequestRows).toHaveBeenCalledTimes(2);
  });
  const asked = starts();
  files.toggle(1);
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(core.pullRequestRows).toHaveBeenCalledTimes(3);
  });

  expect(collapsed).toEqual([false, true, false]);
  expect(shown).toBe(11 + CHUNK);
  expect(asked).toEqual([0, 2 * CHUNK]);
  expect(starts()).toEqual([0, 2 * CHUNK, CHUNK]);
  expect(files.layout.count).toBe(3 * CHUNK);
  files.toggle(1);
  expect(files.collapsed(1)).toBe(true);
});

it("collapses any file on a toggle, names it when asking for spans, and opens it on another", async () => {
  const files = await loaded(withGenerated("abc"));

  files.toggle(0);
  const collapsed = [0, 1, 2].map((at) => files.collapsed(at));
  const shown = files.layout.count;
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(2);
  });
  files.toggle(0);

  expect(collapsed).toEqual([true, true, false]);
  // Two headers and the last file's rows.
  expect(shown).toBe(2 + CHUNK);
  // Opening the file asks again for the spans of the chunk that holds its rows.
  expect(core.pullRequestSpans.mock.calls.map(([chunk, collapsed]) => [chunk.start, collapsed])).toEqual([
    [0, [0, 1]],
    [2 * CHUNK, [0, 1]],
    [0, [1]],
  ]);
  expect(files.collapsed(0)).toBe(false);
  expect(files.layout.count).toBe(11 + CHUNK);
});

it("starts each file afresh when the PR opens again, whatever the user collapsed or opened before", async () => {
  const before = await loaded(withGenerated("abc"));
  before.toggle(0);
  before.toggle(1);

  const again = await loaded(withGenerated("abc"));

  expect([0, 1, 2].map((at) => again.collapsed(at))).toEqual([false, true, false]);
});

it("collapses nothing on a toggle of a file the diff lacks", async () => {
  const files = await loaded(withGenerated("abc"));

  files.toggle(3);

  expect(files.collapsed(3)).toBe(false);
});

const REVIEWED = "1".repeat(40);

/** A Stale review whose check of head `abc` named `paths` as changed, those after a `~` only in
 * whitespace. */
const staleReview = (...paths: string[]): YourReview => staleAt("abc", ...paths);

/** The same, from a check of head `head`. */
const staleAt = (head: string, ...paths: string[]): YourReview => ({
  kind: "stale_review",
  reviewed: REVIEWED,
  head,
  files: paths.map((path) => ({ path: path.replace(/^~/, ""), whitespace_only: path.startsWith("~") })),
});

/** Five files and a Generated one: `Lib/ast.py` changed since the Review, `Lib/os.py` only in
 * whitespace, and `Lib/new.py`, renamed from `Lib/old.py`, changed under its old name. */
const reviewedDiff = () => {
  const renamed = { ...file("Lib/new.py", 40), old_path: "Lib/old.py" };
  return ready("abc", 60, [
    file("Doc/whatsnew.rst", 0),
    file("Lib/ast.py", 10),
    file("Python/generated_cases.c.h", 20, true),
    file("Lib/os.py", 30),
    renamed,
    file("Lib/re.py", 50),
  ]);
};

/** A Files tab for PR 11 that has loaded `answer`, whose PR view says the user's Review is `review`. */
const reviewed = async (answer: PrFiles, review: YourReview | undefined) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, timer());
  files.followReview(review);
  await files.load();
  return files;
};

const ALL = [0, 1, 2, 3, 4, 5];

it("starts the files unchanged since a Stale review folded, and says how each changed one did", async () => {
  const files = await reviewed(reviewedDiff(), staleReview("Lib/ast.py", "~Lib/os.py", "Lib/old.py"));

  expect(ALL.map((at) => files.collapsed(at))).toEqual([true, false, true, false, false, true]);
  expect(ALL.map((at) => files.sinceReview(at))).toEqual([
    "unchanged",
    "changed",
    // The check leaves Generated files out, and so does the tab.
    undefined,
    "whitespace_only",
    "changed",
    "unchanged",
  ]);
});

it.each<[string, YourReview | undefined]>([
  ["Unreviewed", { kind: "unreviewed" }],
  ["Reviewed", { kind: "reviewed" }],
  ["a check that can't run", { kind: "cant_check", reviewed: REVIEWED, reason: { kind: "cloning" } }],
  ["a PR view that hasn't come", undefined],
  // While a newer head waits for its check, the last verdict read an older one, which may miss a change.
  ["a check of another head", staleAt("older", "Lib/ast.py")],
])("folds nothing but Generated files for %s", async (_, review) => {
  const files = await reviewed(reviewedDiff(), review);

  expect(ALL.map((at) => files.collapsed(at))).toEqual([false, false, true, false, false, false]);
  expect(ALL.map((at) => files.sinceReview(at))).toEqual(ALL.map(() => undefined));
  expect(files.foldedUnchanged).toBe(0);
});

it("unfolds every folded unchanged file at once, and leaves Generated files and changed ones the user folded", async () => {
  const files = await reviewed(withGenerated("abc"), staleReview("Lib/ast.py"));
  files.toggle(0);
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(2);
  });
  const folded = files.foldedUnchanged;

  files.unfoldUnchanged();

  expect(folded).toBe(1);
  expect(files.foldedUnchanged).toBe(0);
  expect([0, 1, 2].map((at) => files.collapsed(at))).toEqual([true, true, false]);
  // Opening Lib/os.py asks again for the spans of the chunk that holds its rows, and names the files still
  // collapsed.
  expect(core.pullRequestSpans.mock.calls.map(([chunk, collapsed]) => [chunk.start, collapsed])).toEqual([
    [0, [0, 1, 2]],
    [2 * CHUNK, [0, 1, 2]],
    [2 * CHUNK, [0, 1]],
  ]);
});

it("keeps what the user folded or opened when a newer check names other files", async () => {
  const files = await reviewed(reviewedDiff(), staleReview("Lib/ast.py"));
  files.toggle(5);
  files.toggle(1);

  files.followReview(staleReview("Lib/re.py", "Lib/os.py"));

  expect(ALL.map((at) => files.collapsed(at))).toEqual([true, true, true, false, true, false]);
});

it("asks again for the spans of the chunks it holds when a new verdict opens files, and not for the same one", async () => {
  const files = await reviewed(withGenerated("abc"), staleReview("Lib/ast.py"));
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(2);
  });

  files.followReview(staleReview("Lib/ast.py"));
  const same = core.pullRequestSpans.mock.calls.length;
  files.followReview({ kind: "reviewed" });

  expect(same).toBe(2);
  expect(files.collapsed(2)).toBe(false);
  expect(core.pullRequestSpans.mock.calls.map(([chunk, collapsed]) => [chunk.start, collapsed])).toEqual([
    [0, [1, 2]],
    [2 * CHUNK, [1, 2]],
    [0, [1]],
    [2 * CHUNK, [1]],
  ]);
});

it("moves to whichever of a thread and the changes was asked for last before the diff came", async () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFiles.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const files = new PrFilesState("python/cpython", 11, timer());
  files.followReview(staleReview("Lib/ast.py"));
  const loading = files.load();
  await settled();

  files.showChanges();
  files.reveal("T5");
  answer(ok(threaded()));
  await loading;

  expect(files.jump).toEqual({ id: "T5" });
  core.pullRequestFiles.mockResolvedValue(ok({ state: "fetching" }));
  await files.ask();
  files.reveal("T5");
  files.showChanges();
  core.pullRequestFiles.mockResolvedValue(ok(threaded()));
  await files.ask();
  expect(files.jump).toEqual({ page: { kind: "file", file: 0 } });
});

it("shows the first changed file, opened, and one asked for before the diff came once it does", async () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFiles.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const files = new PrFilesState("python/cpython", 11, timer());
  files.followReview(staleReview("Lib/re.py", "~Lib/os.py"));
  const loading = files.load();
  await settled();

  files.showChanges();
  files.unfoldUnchanged();
  const before = [files.jump, files.sinceReview(0), files.foldedUnchanged];
  answer(ok(reviewedDiff()));
  await loading;

  expect(before).toEqual([undefined, undefined, 0]);
  expect(files.jump).toEqual({ page: { kind: "file", file: 3 } });
  expect(files.sinceReview(6)).toBeUndefined();
  files.toggle(3);
  files.showChanges();
  expect(files.collapsed(3)).toBe(false);
  core.pullRequestFiles.mockResolvedValue(ok(reviewedDiff()));
  const moved = files.jump;
  await files.ask();
  expect(files.jump).toBe(moved);
});

it("shows the top of the diff when no file it holds changed since the Review", async () => {
  const files = await reviewed(reviewedDiff(), staleReview("Lib/gone.py"));
  const empty = await reviewed(ready("abc", 0, []), staleReview("Lib/gone.py"));

  files.showChanges();
  empty.showChanges();

  expect(files.jump).toEqual({ page: { kind: "file", file: 0 } });
  expect(empty.jump).toBeUndefined();
});

it("paints the answer to the latest ask for a chunk's spans, whichever comes last", async () => {
  const files = await loaded(withGenerated("abc"));
  files.toggle(0);
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(0));
  });
  const answerOlder = heldSpans();

  // Each asks again for the first chunk's spans: the first while Lib/ast.py is still collapsed.
  files.toggle(1);
  files.toggle(0);
  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(4);
  });
  answerOlder(ok([keyword(99)]));
  await settled();

  expect(files.spans(0)).toEqual(keyword(0));
});

it("asks again for the spans of the chunks that hold a collapsed file's rows once it opens, naming those still collapsed", async () => {
  // Spans that count the files the window shows collapsed, so the test can tell an answer to a later ask.
  core.pullRequestSpans.mockImplementation(({ start }, collapsed = []) =>
    Promise.resolve(ok([keyword(start + collapsed.length)])),
  );
  const files = await loaded(withGenerated("abc"));
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(files.spans(2 * CHUNK)).toEqual(keyword(2 * CHUNK + 1));
  });

  files.toggle(1);
  files.need(0, 20);

  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(4);
  });
  // The last file's chunk holds none of the Generated file's rows, so its spans stand.
  const asked = core.pullRequestSpans.mock.calls.map(([chunk, collapsed]) => [chunk.start, collapsed]);
  expect(asked).toEqual([
    [0, [1]],
    [2 * CHUNK, [1]],
    [0, []],
    [CHUNK, []],
  ]);
  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(0));
  });
});

it("keeps a chunk's spans when asking again fails, and drops those that come once it's gone", async () => {
  // The Generated file's rows reach into the last file's first chunk.
  const diff = (head: string) =>
    ready(head, 3 * CHUNK, [
      file("Lib/ast.py", 0),
      file("Python/generated_cases.c.h", 10, true),
      file("Lib/os.py", 2 * CHUNK + 10),
    ]);
  const files = await loaded(diff("abc"));
  files.need(0, 5);
  await vi.waitFor(() => {
    expect(files.spans(2 * CHUNK)).toEqual(keyword(2 * CHUNK));
  });
  core.pullRequestSpans.mockResolvedValueOnce({ status: "error", error: { message: "No tree" } });
  const answerLate = heldSpans();

  files.toggle(1);
  core.pullRequestFiles.mockResolvedValue(ok(diff("def")));
  const failed = files.spans(0);
  await files.ask();
  answerLate(ok([keyword(99)]));

  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(4);
  });
  expect(failed).toEqual(keyword(0));
  expect(files.spans(2 * CHUNK)).toBeUndefined();
});

it("collapses the Generated files of a new head again", async () => {
  const files = await loaded(withGenerated("abc"));
  files.toggle(1);
  core.pullRequestFiles.mockResolvedValue(ok(withGenerated("def")));

  await files.ask();

  expect(files.collapsed(1)).toBe(true);
});

it("starts over when the PR moves to another branch with the same head, and drops the old rows late", async () => {
  const diff = (base: string) => ready("abc", 10, [file("NEWS", 0), file("Lib/generated.py", 5, true)], base);
  const files = await loaded(diff("on-main"));
  files.toggle(1);
  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("row 0"));
  });
  const answerLate = heldRows();
  core.pullRequestFiles.mockResolvedValue(ok(diff("on-3.14")));
  await files.ask();
  files.need(0, 9);
  core.pullRequestFiles.mockResolvedValue(ok(diff("on-3.13")));
  await files.ask();
  const answerNew = heldRows();
  files.need(0, 9);
  answerLate(ok([added("from 3.14")]));
  await settled();

  expect(files.row(0)).toBeUndefined();
  expect(files.collapsed(1)).toBe(true);
  const bases = core.pullRequestRows.mock.calls.map(([chunk]) => (chunk as { base?: string }).base);
  expect(bases).toEqual(["on-main", "on-3.14", "on-3.13"]);
  answerNew(ok([added("from 3.13")]));
  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("from 3.13"));
  });
});

it("lets go of the chunks far from what shows, and of those that arrive once far", async () => {
  const files = await loaded(ready("abc", 20 * CHUNK));
  files.need(0, 10);
  await vi.waitFor(() => {
    expect(files.row(CHUNK)).toBeDefined();
  });
  let answerLate: (answer: unknown) => void = () => {};
  core.pullRequestRows.mockReturnValueOnce(
    new Promise((resolve) => {
      answerLate = resolve;
    }),
  );
  files.need(5 * CHUNK, 5 * CHUNK + 10);

  // The chunk before what shows, and the one after the next, stay.
  files.need(10 * CHUNK, 10 * CHUNK + 10);
  answerLate(ok([added("late")]));
  await vi.waitFor(() => {
    expect(files.row(11 * CHUNK)).toBeDefined();
  });
  expect(files.row(0)).toBeUndefined();
  expect(files.row(CHUNK)).toBeUndefined();
  expect(files.row(5 * CHUNK)).toBeUndefined();

  files.need(0, 10);
  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("row 0"));
  });
  files.need(CHUNK + 10, CHUNK + 20);
  await vi.waitFor(() => {
    expect(core.pullRequestRows).toHaveBeenCalledTimes(9);
  });
});

it("keeps asking for a chunk of the newer diff when the same chunk of the older one fails", async () => {
  const diff = (base: string) => ready("abc", 10, [file("NEWS", 0)], base);
  const files = await loaded(diff("on-main"));
  const failOld = heldRows();
  files.need(0, 9);
  core.pullRequestFiles.mockResolvedValue(ok(diff("on-3.14")));
  await files.ask();
  const answerNew = heldRows();
  files.need(0, 9);

  failOld({ status: "error", error: { message: "gone" } });
  await settled();
  answerNew(ok([added("on 3.14")]));

  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("on 3.14"));
  });
});

it("asks for a chunk again after it failed", async () => {
  const files = await loaded(ready("abc", 10));
  core.pullRequestRows.mockResolvedValueOnce({
    status: "error",
    error: { message: '"abc" isn\'t a commit.' },
  });

  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.error).toBe('"abc" isn\'t a commit.');
  });
  files.need(0, 9);

  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("row 0"));
  });
});

it("forgets the rows of an older head, and drops them if they arrive late", async () => {
  const files = await loaded(ready("old", 10));
  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("row 0"));
  });
  let answerLate: (answer: unknown) => void = () => {};
  core.pullRequestRows.mockReturnValueOnce(
    new Promise((resolve) => {
      answerLate = resolve;
    }),
  );
  core.pullRequestFiles.mockResolvedValue(ok(ready("old", 20)));
  await files.ask();
  // The same head keeps its rows.
  expect(files.row(0)).toEqual(added("row 0"));

  core.pullRequestFiles.mockResolvedValue(ok(ready("new", 10)));
  await files.ask();
  expect(files.row(0)).toBeUndefined();
  files.need(0, 9);
  core.pullRequestFiles.mockResolvedValue(ok(ready("newer", 10)));
  await files.ask();
  answerLate(ok([added("from new")]));

  await vi.waitFor(() => {
    expect(core.pullRequestRows).toHaveBeenCalledTimes(2);
  });
  expect(files.row(0)).toBeUndefined();
});

/** How many lines the fake core's files have at the head. */
const LENGTH = 70;

/** The lines from `start` up to `end` of a file `LENGTH` lines long, as the fake core sends them. */
const headLines = (start: number, end: number) =>
  Array.from({ length: Math.max(0, Math.min(end, LENGTH + 1) - start) }, (_, at) => ({
    text: `line ${String(start + at)}`,
    spans: [],
  }));

/** A diff of one file, whose hunks start at rows 1 and 6: 5 lines left out above the first and 45 above
 * the second, which sit 2 lines further down at the head. Lines may follow the last hunk. A second file,
 * from row 20, ends with its only hunk. */
const gapped = (base = "base") =>
  ready(
    "abc",
    30,
    [
      {
        ...file("a.py", 0),
        gaps: [
          { row: 1, old: 1, new: 1, lines: 5 },
          { row: 6, old: 10, new: 12, lines: 45 },
        ],
        rest: { old: 60, new: 62 },
        pairs: [],
      },
      file("b.py", 20),
    ],
    base,
  );
const [small, large] = [
  { file: 0, old: 1, new: 1, lines: 5 },
  { file: 0, old: 10, new: 12, lines: 45 },
];

/** The lines the gap above the hunk at row `row` still hides nearest it, which its buttons show. */
const hiddenAt = (files: PrFilesState, row: number) => files.layout.gapAt(row) ?? large;

/** Where each ask for lines started and ended. */
const asked = () => core.pullRequestLines.mock.calls.map(([lines]) => [lines.start, lines.end]);

it("shows 20 more lines above a hunk or below the one before, and then the rest of a short gap", async () => {
  const files = await loaded(gapped());

  files.expandAbove(hiddenAt(files, 6));
  await vi.waitFor(() => {
    expect(files.line(0, 56)).toEqual({ text: "line 56", spans: [] });
  });
  files.expandBelow(hiddenAt(files, 6));
  await vi.waitFor(() => {
    expect(files.layout.gapAt(6)?.lines).toBe(45 - 2 * EXPAND);
  });
  files.expandGap(hiddenAt(files, 6));
  await vi.waitFor(() => {
    expect(files.layout.gapAt(6)).toBeUndefined();
  });

  expect(asked()).toEqual([
    [37, 57],
    [12, 32],
    [32, 37],
  ]);
  // The hunk's row goes, and the gap's 45 lines show.
  expect(files.layout.count).toBe(30 + 1 - 1 + 45);
  expect(files.layout.at(6)).toEqual({ kind: "line", file: 0, old: 10, new: 12 });
  expect(files.columns).toBe("line 56".length);
});

it("stops a gap's lines at its ends, and asks for none once it shows them all", async () => {
  const files = await loaded(gapped());

  files.expandBelow(small);
  await vi.waitFor(() => {
    expect(files.layout.gapAt(1)).toBeUndefined();
  });
  files.expandAbove(small);
  files.expandGap(small);

  expect(asked()).toEqual([[1, 6]]);
});

it("shows 20 more lines after a file's last hunk, and drops the button once the file ends", async () => {
  const files = await loaded(gapped());
  core.pullRequestLines.mockResolvedValueOnce(ok(headLines(1, 1 + EXPAND)));

  await files.expandRest(0);
  expect(files.layout.at(20 + EXPAND)).toEqual({ kind: "more", file: 0 });
  await files.expandRest(0);

  expect(asked()).toEqual([
    [62, 82],
    [82, 102],
  ]);
  // The second ask came back empty, so the file ends after the first 20.
  expect(files.layout.count).toBe(30 + EXPAND);
  expect(files.layout.at(20 + EXPAND)).toEqual({ kind: "row", row: 20 });
});

it("shows a whole file in one ask, every gap and the lines after its last hunk", async () => {
  const files = await loaded(gapped());

  await files.expandFile(0);
  await files.expandFile(1);

  expect(asked()).toEqual([
    [1, 2 ** 32 - 1],
    [1, 2 ** 32 - 1],
  ]);
  // Both hunk rows go, and the 5 and 45 lines of the gaps and the 9 to line 70 show.
  expect(files.layout.count).toBe(30 - 2 + 5 + 45 + (LENGTH - 62 + 1));
  expect(files.line(0, 70)).toEqual({ text: "line 70", spans: [] });
});

it("forgets what it expanded on a new diff, and drops lines that come for an older one", async () => {
  const files = await loaded(gapped("on-main"));
  files.expandAbove(large);
  await vi.waitFor(() => {
    expect(files.line(0, 56)).toEqual({ text: "line 56", spans: [] });
  });
  let answerLate: (answer: unknown) => void = () => {};
  core.pullRequestLines.mockReturnValueOnce(
    new Promise((resolve) => {
      answerLate = resolve;
    }),
  );
  const late = files.expandRest(0);
  core.pullRequestFiles.mockResolvedValue(ok(gapped("on-3.14")));
  await files.ask();
  answerLate(ok(headLines(62, 82)));
  await late;

  expect(files.line(0, 56)).toEqual({ text: "", spans: [] });
  expect(files.line(0, 62)).toEqual({ text: "", spans: [] });
  expect(files.layout.gapAt(6)?.lines).toBe(45);
  expect(files.layout.at(20)).toEqual({ kind: "more", file: 0 });
});

it("keeps the button when the core can't send lines, and says why", async () => {
  const files = await loaded(gapped());
  const failed = { status: "error", error: { message: "gone" } };
  core.pullRequestLines.mockResolvedValue(failed);

  files.expandAbove(large);
  await files.expandRest(0);
  await files.expandFile(0);

  expect(files.error).toBe("gone");
  expect(files.layout.at(20)).toEqual({ kind: "more", file: 0 });
  expect(files.layout.gapAt(6)?.lines).toBe(45);
});

it("expands nothing until the diff is ready, nor after a file that ends with its last hunk", async () => {
  const files = await loaded();

  files.expandAbove(large);
  await files.expandRest(0);
  await files.expandFile(0);
  core.pullRequestFiles.mockResolvedValue(ok(gapped()));
  await files.ask();
  await files.expandRest(1);

  expect(asked()).toEqual([]);
});

it("lays the diff out in the view the window chose, which one choice holds for every PR", async () => {
  const choice = new DiffViewChoice();
  const changed = { ...file("a.py", 0), pairs: [{ row: 2, removed: 2, added: 1 }] };
  core.pullRequestFiles.mockResolvedValue(ok(ready("abc", 6, [changed])));
  const one = new PrFilesState("python/cpython", 11, timer(), choice);
  const other = new PrFilesState("python/cpython", 12, timer(), choice);
  await Promise.all([one.load(), other.load()]);
  const unified = one.layout.count;

  choice.toggle();

  expect([unified, one.layout.count, other.layout.count]).toEqual([6, 5, 5]);
  expect([one.shownAs, other.shownAs]).toEqual(["side-by-side", "side-by-side"]);
  choice.toggle();
  expect(one.shownAs).toBe("unified");
});

/** An open thread `id` at `at`, a row of the diff when a number, with `comments` comments. */
const thread = (
  id: string,
  at: number | DiffThread["at"],
  resolution: "open" | "resolved" = "open",
  comments = 1,
): DiffThread => ({
  id,
  at: typeof at === "number" ? { kind: "row", row: at } : at,
  resolution,
  comments: Array.from({ length: comments }, () => ({
    id: null,
    author: "some-reviewer",
    created_at: "2026-11-04T10:00:00Z",
    body_html: "<p>Why?</p>",
    hidden: null,
    submission: "submitted",
  })),
  later_comments: 0,
});

/** The threads `threads`, as the core sends them with a diff. */
const shown = (...threads: DiffThread[]) => ({ kind: "shown" as const, threads });

/** A diff of 40 rows: a file, a Generated one from row 20, and threads under rows 2, 5, 9 and 25, the
 * second resolved. */
const threaded = (): Extract<PrFiles, { state: "ready" }> => ({
  ...ready("abc", 40, [file("Lib/ast.py", 0), file("Python/generated.c.h", 20, true)]),
  threads: shown(thread("T2", 2, "open", 2), thread("T5", 5, "resolved"), thread("T9", 9), thread("T25", 25)),
});

/** The places each thread takes, by its id, and `-` for one that doesn't show. */
const threadPlaces = (files: PrFilesState) =>
  files.threads.map((shown) => {
    const place = files.layout.threadAt(shown.id);
    return place === undefined ? `${shown.id} -` : `${shown.id} ${String(place)}`;
  });

it("folds a resolved thread to a line and leaves an open one open, as the user turns either", async () => {
  const files = await loaded(threaded());

  expect([0, 1, 2, 4].map((at) => files.folded(at))).toEqual([false, true, false, false]);
  // A guess until the tab measures it: a header, and three places for each comment.
  expect(files.layout.threadsIn(0, 99).map((placed) => placed.length)).toEqual([7, 1, 4]);
  files.toggleThread(0);
  files.toggleThread(1);
  files.toggleThread(9);
  expect([0, 1].map((at) => files.folded(at))).toEqual([true, false]);
  files.measured("T5", 3);
  files.measured("T5", 3);
  files.measured("T2", 9);
  expect(files.layout.threadsIn(0, 99).map((placed) => placed.length)).toEqual([1, 3, 4]);
  files.toggleThread(0);
  expect(files.layout.threadsIn(0, 99).map((placed) => placed.length)).toEqual([9, 3, 4]);
});

it("moves through the open threads with F8 and back with Shift+F8, round from either end", async () => {
  const files = await loaded(threaded());
  const visited: string[] = [];
  const press = (step: 1 | -1) => {
    files.nextThread(step);
    const jump = files.jump;
    visited.push(jump !== undefined && "id" in jump ? jump.id : "none");
  };

  press(1);
  press(1);
  press(1);
  press(1);
  press(-1);
  expect(visited).toEqual(["T2", "T9", "T25", "T2", "T25"]);
  // The thread under row 25 is in the Generated file, which the move opened.
  expect(files.collapsed(1)).toBe(false);
  expect(threadPlaces(files).at(-1)).toBe("T25 38");
});

it("starts from the last thread going back, and goes nowhere without an open thread", async () => {
  const files = await loaded(threaded());
  files.nextThread(-1);
  expect(files.jump).toEqual({ id: "T25" });
  // A new diff without the thread starts from the end again.
  core.pullRequestFiles.mockResolvedValue(
    ok({ ...threaded(), threads: shown(thread("T2", 2), thread("T9", 9)) }),
  );
  await files.ask();
  files.nextThread(1);
  expect(files.jump).toEqual({ id: "T2" });

  const none = await loaded({ ...threaded(), threads: shown(thread("T5", 5, "resolved")) });
  none.nextThread(1);
  expect(none.jump).toBeUndefined();
});

it("reveals a thread open, even a resolved one, and one asked for before the diff came once it does", async () => {
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFiles.mockReturnValue(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const files = new PrFilesState("python/cpython", 11, timer());
  const loading = files.load();
  await settled();

  files.reveal("T5");
  expect(files.jump).toBeUndefined();
  answer(ok(threaded()));
  await loading;

  expect(files.jump).toEqual({ id: "T5" });
  expect(files.folded(1)).toBe(false);
  files.reveal("T2");
  const moved = files.jump;
  files.reveal("gone");
  expect(files.jump).toBe(moved);
  core.pullRequestFiles.mockResolvedValue(ok(threaded()));
  await files.ask();
  expect(files.jump).toBe(moved);
});

it("has no threads before the diff comes", async () => {
  const files = await loaded();

  expect(files.threads).toEqual([]);
  expect(files.folded(0)).toBe(false);
  expect(files.heldBack).toBeUndefined();
});

/** The code an Outdated thread was written against, `rows` rows of it. */
const code = (rows: number) => ({
  rows: Array.from({ length: rows }, (_, at) => ({ kind: "context" as const, old: at, new: at, text: "x" })),
  spans: Array.from({ length: rows }, () => []),
});

it("puts a file's own threads at its top, its Outdated ones after them, and threads on no file at the end", async () => {
  const files = await loaded({
    ...threaded(),
    threads: shown(
      thread("whole", { kind: "file", file: 1 }),
      thread("outdated", { kind: "outdated", file: 1, code: code(9) }),
      thread("bare", { kind: "outdated", file: 1, code: null }),
      thread("in it", 25),
      thread("gone", { kind: "gone", path: "Lib/old.py", code: null }),
    ),
  });
  files.toggle(1);

  // Row 20 heads the Generated file, and the heading of the threads on no file follows the last row.
  expect(threadPlaces(files)).toEqual(["whole 21", "outdated 25", "bare 34", "in it 43", "gone 62"]);
  expect(files.layout.at(61)).toEqual({ kind: "gone" });
  // A guess until the tab measures it: a header, the last rows of its code and a line above them, and three
  // places for each comment.
  expect(files.layout.threadsIn(0, 99).map((placed) => placed.length)).toEqual([4, 9, 4, 4, 4]);
});

it("puts a thread on a file the diff doesn't have at the top, should a payload name one", async () => {
  const files = await loaded({ ...threaded(), threads: shown(thread("lost", { kind: "file", file: 9 })) });

  expect(threadPlaces(files)).toEqual(["lost 1"]);
});

it("reveals a thread at the top of a collapsed file, opening the file, and one on no file", async () => {
  const files = await loaded({
    ...threaded(),
    threads: shown(
      thread("outdated", { kind: "outdated", file: 1, code: null }),
      thread("gone", { kind: "gone", path: "Lib/old.py", code: null }),
    ),
  });

  files.reveal("outdated");
  expect([files.jump, files.collapsed(1)]).toEqual([{ id: "outdated" }, false]);
  files.reveal("gone");
  expect(files.jump).toEqual({ id: "gone" });
});

it("opens the lines around a thread on a line no hunk shows, once, and its thread sits under the line", async () => {
  const outside = thread("outside", { kind: "outside", file: 0, line: 28, head_line: 30 });
  const files = await loaded({ ...gapped(), threads: shown(outside) });
  await vi.waitFor(() => {
    expect(files.line(0, 33)).toEqual({ text: "line 33", spans: [] });
  });
  await files.ask();
  core.pullRequestFiles.mockResolvedValue(
    ok({
      ...gapped(),
      threads: shown(outside, thread("near", { kind: "outside", file: 0, line: 1, head_line: 2 })),
    }),
  );
  await files.ask();

  expect(asked()).toEqual([
    [27, 34],
    [1, 6],
  ]);
  // Lines 12 to 26 hide above the window, under their buttons, and the rest of the gap below it.
  expect(files.layout.at(6)).toEqual({ kind: "hidden", file: 0, old: 10, new: 12, lines: 15 });
  expect(files.layout.at(10)).toEqual({ kind: "line", file: 0, old: 28, new: 30 });
  expect(files.layout.at(11)).toEqual({ kind: "thread", id: "outside", into: 0 });
});

it("shows the diff before a moved head with its threads held back, then the newer diff at the same lines", async () => {
  const before = { ...gapped("older"), threads: { kind: "held_back" as const, newer: "newer", count: 3 } };
  const files = await loaded(before);

  expect([files.threads, files.heldBack]).toEqual([[], { newer: "newer", count: 3 }]);
  core.pullRequestFiles.mockResolvedValue(ok(gapped("newer")));
  await files.ask();
  expect([files.heldBack, files.layout.diff]).toEqual([undefined, "newer..abc"]);
});

it("says why the fetch of a moved head failed until it asks again, which fetches it once more", async () => {
  const before = { ...gapped("older"), threads: { kind: "held_back" as const, newer: "newer", count: 3 } };
  const files = await loaded(before);
  const unreachable = {
    kind: "git" as const,
    words: "fatal: unable to access 'https://github.com/python/cpython.git/': Could not resolve host",
  };
  const other = { kind: "git" as const, words: "other" };

  core.heard.headsFetchFailed?.({ payload: { repository: "python/peps", items: [11], why: other } });
  core.heard.headsFetchFailed?.({ payload: { repository: "python/cpython", items: [12], why: other } });
  expect(files.unfetched).toBeUndefined();
  core.heard.headsFetchFailed?.({
    payload: { repository: "python/cpython", items: [11, 12], why: unreachable },
  });
  expect([files.unfetched, files.heldBack]).toEqual([unreachable, { newer: "newer", count: 3 }]);
  const asking = files.ask();
  // The core hands the head to the mirror again as it answers.
  expect(files.unfetched).toBeUndefined();
  await asking;
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(2);
  core.pullRequestFiles.mockResolvedValue(ok(gapped("newer")));
  core.heard.headsFetched?.({ payload: { repository: "python/cpython", items: [11] } });
  await vi.waitFor(() => {
    expect([files.heldBack, files.layout.diff]).toEqual([undefined, "newer..abc"]);
  });
});

it("hears that GitHub's ref for the PR is elsewhere without asking again, so no fetch follows", async () => {
  const files = await loaded({ state: "fetching" });

  core.heard.headsFetchFailed?.({
    payload: { repository: "python/cpython", items: [11], why: { kind: "behind" } },
  });

  expect(files.unfetched).toEqual({ kind: "behind" });
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(1);
});

it("reveals a thread asked for while the older diff holds its threads back, once the newer diff comes", async () => {
  const files = await loaded({ ...threaded(), threads: { kind: "held_back", newer: "newer", count: 4 } });

  files.reveal("T9");
  expect(files.jump).toBeUndefined();
  core.pullRequestFiles.mockResolvedValue(ok(threaded()));
  await files.ask();
  expect(files.jump).toEqual({ id: "T9" });
});

/** A diff of 40 rows: `Lib/ast.py` with changes from rows 2 and 6, and a Generated file from row 20 with one
 * from row 23. */
const changed = () =>
  ready("abc", 40, [
    {
      ...file("Lib/ast.py", 0),
      pairs: [
        { row: 2, removed: 1, added: 1 },
        { row: 6, removed: 0, added: 2 },
      ],
    },
    { ...file("Python/generated.c.h", 20, true), pairs: [{ row: 23, removed: 1, added: 0 }] },
  ]);

/** Where the tab moves on each of `times` presses of F7, or of Shift+F7 for `step` -1. */
const changes = (files: PrFilesState, step: 1 | -1, times: number) =>
  Array.from({ length: times }, () => {
    files.nextChange(step);
    return files.jump;
  });

it("moves to the next change across files with F7, opening a Generated file, and round from the end", async () => {
  const files = await loaded(changed());

  expect(changes(files, 1, 4)).toEqual([{ row: 2 }, { row: 6 }, { row: 23 }, { row: 2 }]);
  expect(files.collapsed(1)).toBe(false);
  expect(changes(files, -1, 2)).toEqual([{ row: 23 }, { row: 6 }]);
});

it("opens a file the user collapsed when F7 moves into it", async () => {
  const files = await loaded(changed());
  files.toggle(0);

  expect(changes(files, 1, 1)).toEqual([{ row: 2 }]);
  expect(files.collapsed(0)).toBe(false);
});

it("moves to the last change first with Shift+F7, and nowhere in a diff without changes or before it comes", async () => {
  expect(changes(await loaded(changed()), -1, 1)).toEqual([{ row: 23 }]);
  expect(changes(await loaded(ready("abc", 3)), 1, 1)).toEqual([undefined]);
  expect(changes(await loaded(), 1, 1)).toEqual([undefined]);
});

/** The core's answer to a search, which found `matches`, each a row and where in it the text starts and
 * ends. */
const found = (matches: [number, number, number][], more = false) =>
  ok({ matches: matches.map(([row, start, end]) => ({ row, start, end })), more });

it("finds text in every row through the core, moves to the first match, and goes round the matches", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValue(
    found([
      [3, 0, 2],
      [3, 4, 6],
      [24, 1, 3],
    ]),
  );

  files.openSearch();
  await files.find("ab");

  expect(core.pullRequestFind).toHaveBeenCalledExactlyOnceWith({
    repository: "python/cpython",
    number: 11,
    head: "abc",
    base: "base",
    query: "ab",
  });
  expect([files.searching, files.query, files.search, files.jump]).toEqual([
    true,
    "ab",
    { at: 0, count: 3, more: false },
    { row: 3 },
  ]);
  expect([files.foundIn(3), files.foundIn(4)]).toEqual([
    [
      { start: 0, end: 2, current: true },
      { start: 4, end: 6, current: false },
    ],
    [],
  ]);
  files.nextMatch(1);
  files.nextMatch(1);
  expect([files.search?.at, files.jump, files.collapsed(1)]).toEqual([2, { row: 24 }, false]);
  files.nextMatch(1);
  expect(files.search?.at).toBe(0);
  files.nextMatch(-1);
  expect(files.search?.at).toBe(2);
});

it("keeps to the match it was on, or the first after it, as the query grows", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValueOnce(
    found([
      [3, 0, 1],
      [9, 0, 1],
      [24, 0, 1],
    ]),
  );
  await files.find("a");
  files.nextMatch(1);

  core.pullRequestFind.mockResolvedValueOnce(
    found([
      [3, 0, 2],
      [12, 0, 2],
    ]),
  );
  await files.find("ab");

  expect([files.search?.at, files.jump]).toEqual([1, { row: 12 }]);
});

it("says when it finds nothing or more than the core sends, and an empty query asks nothing", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValueOnce(found([]));
  await files.find("zz");
  const none = files.search;
  // Nowhere to move to.
  files.nextMatch(1);
  expect(files.jump).toBeUndefined();
  core.pullRequestFind.mockResolvedValueOnce(found([[3, 0, 1]], true));
  await files.find("z");
  const more = files.search;

  await files.find("");
  files.nextMatch(1);

  expect([none, more, files.search]).toEqual([
    { at: undefined, count: 0, more: false },
    { at: 0, count: 1, more: true },
    undefined,
  ]);
  expect(core.pullRequestFind).toHaveBeenCalledTimes(2);
});

it("drops the answer to an older query that comes after a newer one", async () => {
  const files = await loaded(changed());
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFind.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  const older = files.find("a");
  core.pullRequestFind.mockResolvedValueOnce(found([[9, 0, 2]]));
  await files.find("ab");

  answer(found([[3, 0, 1]]));
  await older;

  expect([files.query, files.search?.count, files.jump]).toEqual(["ab", 1, { row: 9 }]);
});

it("searches the newer diff again for the same text, and asks nothing before a diff comes", async () => {
  const files = await loaded();
  await files.find("ab");
  expect(core.pullRequestFind).not.toHaveBeenCalled();

  core.pullRequestFind.mockResolvedValue(found([[3, 0, 2]]));
  core.pullRequestFiles.mockResolvedValue(ok(changed()));
  await files.ask();
  await vi.waitFor(() => {
    expect(files.search?.count).toBe(1);
  });
  await files.ask();
  expect(core.pullRequestFind).toHaveBeenCalledTimes(1);
});

it("says in the search why it failed, and leaves the tab's error to the diff", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "gone" },
  });
  await files.find("ab");
  const failed = [files.error, files.searchProblem, files.search];
  core.pullRequestFind.mockRejectedValueOnce(new Error("no core"));
  await files.find("abc");
  const rejected = files.searchProblem;

  core.pullRequestFind.mockResolvedValueOnce(found([[3, 0, 2]]));
  await files.find("ab");

  expect(failed).toEqual([undefined, "gone", undefined]);
  expect(rejected).toBe("Error: no core");
  expect([files.searchProblem, files.search?.count]).toEqual([undefined, 1]);
});

it("searches a new diff again without moving, and drops the matches of the diff before at once", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValueOnce(found([[24, 0, 2]]));
  await files.find("ab");
  files.jump = undefined;
  let answer: (answer: unknown) => void = () => {};
  core.pullRequestFind.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );

  core.pullRequestFiles.mockResolvedValue(ok({ ...changed(), head: "def" }));
  await files.ask();
  const meanwhile = [files.search, files.foundIn(24)];
  answer(found([[3, 0, 2]]));
  await vi.waitFor(() => {
    expect(files.search?.count).toBe(1);
  });

  expect(meanwhile).toEqual([undefined, []]);
  expect([files.search?.at, files.foundIn(3), files.jump]).toEqual([
    0,
    [{ start: 0, end: 2, current: true }],
    undefined,
  ]);
});

it("closes the search, forgetting its text and its matches", async () => {
  const files = await loaded(changed());
  core.pullRequestFind.mockResolvedValue(found([[3, 0, 2]]));
  files.openSearch();
  await files.find("ab");

  files.closeSearch();

  expect([files.searching, files.query, files.search, files.foundIn(3)]).toEqual([false, "", undefined, []]);
  expect(files.diffWantsFocus).toBe(true);
});

/** A Files tab for PR 11 that shows one file at a time, as `[diff] files = "one"` opens it. */
const oneAtATime = async (answer: PrFiles) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, timer(), new DiffViewChoice(), "one");
  await files.load();
  return files;
};

/** A NEWS entry added at `path`. */
const newsEntry = (path: string) => ({ path, section: "Library", issue: null, blocks: [] });

/** The layout's places: a row by its number, a thread once by its id, and anything else by its kind. */
const placesOf = (files: PrFilesState) =>
  Array.from({ length: files.layout.count }, (_, place) => files.layout.at(place)).flatMap(
    (at): (string | number)[] => {
      if (at.kind === "thread") return at.into === 0 ? [at.id] : [];
      return [at.kind === "row" ? at.row : at.kind];
    },
  );

it("opens on all the files unless the settings say one at a time", async () => {
  const files = await loaded(changed());

  expect([files.mode, files.shown]).toEqual(["all", undefined]);
});

it("opens one file at a time on the NEWS entries, then walks the files in order and stops at either end", async () => {
  const files = await oneAtATime({
    ...ready("abc", 6, [file("Lib/ast.py", 0), file("Misc/NEWS.d/next/x.rst", 3)]),
    news: [newsEntry("Misc/NEWS.d/next/x.rst")],
  });

  expect([files.mode, files.shown, files.layout.count]).toEqual(["one", { kind: "news" }, 0]);
  files.nextFile(-1);
  expect([files.shown, files.jump]).toEqual([{ kind: "news" }, undefined]);
  files.nextFile(1);
  expect([files.shown, files.jump, placesOf(files)]).toEqual([
    { kind: "file", file: 0 },
    { page: { kind: "file", file: 0 } },
    [0, 1, 2],
  ]);
  files.nextFile(1);
  files.nextFile(1);
  expect([files.shown, placesOf(files)]).toEqual([{ kind: "file", file: 1 }, [3, 4, 5]]);
  files.nextFile(-1);
  expect(files.shown).toEqual({ kind: "file", file: 0 });
});

it("gives a Generated file its turn, collapsed to its header", async () => {
  const files = await oneAtATime(changed());

  files.nextFile(1);

  expect([files.shown, placesOf(files)]).toEqual([{ kind: "file", file: 1 }, [20]]);
});

it("shows the threads on files the PR no longer changes on a last page of their own", async () => {
  const files = await oneAtATime({
    ...threaded(),
    threads: shown(thread("T2", 2), thread("gone", { kind: "gone", path: "Lib/old.py", code: null })),
  });

  files.nextFile(1);
  files.nextFile(1);

  expect([files.shown, placesOf(files)]).toEqual([{ kind: "gone" }, ["gone", "gone"]]);
  files.reveal("T2");
  expect([files.shown, placesOf(files)]).toEqual([
    { kind: "file", file: 0 },
    [0, 1, 2, "T2", ...range(3, 20)],
  ]);
  files.reveal("gone");
  expect(files.shown).toEqual({ kind: "gone" });
});

/** The numbers from `start` up to `end`. */
const range = (start: number, end: number) => Array.from({ length: end - start }, (_, at) => start + at);

it("goes to the header of the file after the one at the top in all files, and back to the NEWS entries", async () => {
  const files = await loaded({ ...changed(), news: [newsEntry("Misc/NEWS.d/next/x.rst")] });

  files.scrolled(-2);
  files.nextFile(-1);
  expect(files.jump).toBeUndefined();
  files.nextFile(1);
  expect(files.jump).toEqual({ page: { kind: "file", file: 0 } });
  files.scrolled(files.layout.placeOfRow(7) ?? -1);
  files.nextFile(1);
  expect(files.jump).toEqual({ page: { kind: "file", file: 1 } });
  files.nextFile(-1);
  expect(files.jump).toEqual({ page: { kind: "news" } });
  // Past the last place, as a scroll can land while the layout shrinks, it starts from the first page.
  files.scrolled(999);
  files.nextFile(1);
  expect(files.jump).toEqual({ page: { kind: "file", file: 0 } });
});

it("switches to one file at a time on the file at the top, and back, for as long as the PR stays open", async () => {
  const files = await loaded(changed());
  files.scrolled(files.layout.placeOfRow(20) ?? -1);

  files.toggleMode();
  expect([files.mode, files.shown, placesOf(files)]).toEqual(["one", { kind: "file", file: 1 }, [20]]);
  files.toggleMode();
  expect([files.mode, files.shown, files.layout.count]).toEqual(["all", undefined, 21]);
});

it("carries F7 on into the next file, showing it, and goes on from a file a move shows", async () => {
  const files = await oneAtATime(changed());

  expect(changes(files, 1, 3)).toEqual([{ row: 2 }, { row: 6 }, { row: 23 }]);
  expect([files.shown, files.collapsed(1)]).toEqual([{ kind: "file", file: 1 }, false]);
  files.nextFile(-1);
  expect(changes(files, 1, 1)).toEqual([{ row: 2 }]);
  files.showFile(1);
  expect(changes(files, -1, 1)).toEqual([{ row: 6 }]);
  expect(files.shown).toEqual({ kind: "file", file: 0 });
});

it("carries F8 on into the next file, and goes on from a file a move shows", async () => {
  const files = await oneAtATime(threaded());

  files.nextThread(1);
  files.nextThread(1);
  files.nextThread(1);
  expect([files.jump, files.shown]).toEqual([{ id: "T25" }, { kind: "file", file: 1 }]);
  files.showFile(1);
  files.nextThread(-1);
  expect([files.jump, files.shown]).toEqual([{ id: "T9" }, { kind: "file", file: 0 }]);
  files.showFile(1);
  files.nextThread(1);
  expect(files.jump).toEqual({ id: "T25" });
});

it("keeps the layout as F7 moves within the file on show", async () => {
  const files = await oneAtATime(changed());
  files.nextChange(1);
  const layout = files.layout;

  files.nextChange(1);

  expect(files.jump).toEqual({ row: 6 });
  expect(files.layout).toBe(layout);
});

it("starts F8 from the first thread of a newer diff, not from the page of the one before", async () => {
  const files = await oneAtATime(threaded());
  files.showFile(1);

  core.pullRequestFiles.mockResolvedValue(ok({ ...threaded(), head: "def" }));
  await files.ask();
  files.nextThread(1);

  expect(files.jump).toEqual({ id: "T2" });
});

it("starts F8 from the first thread after a move to the NEWS entries", async () => {
  const files = await oneAtATime({ ...threaded(), news: [newsEntry("Misc/NEWS.d/next/x.rst")] });
  files.reveal("T25");

  files.nextFile(-1);
  files.nextFile(-1);
  files.nextThread(1);

  expect([files.jump, files.shown]).toEqual([{ id: "T2" }, { kind: "file", file: 0 }]);
});

it("finds text across the whole PR, showing the file of each match", async () => {
  const files = await oneAtATime(changed());
  core.pullRequestFind.mockResolvedValue(
    found([
      [3, 0, 2],
      [24, 1, 3],
    ]),
  );

  await files.find("ab");
  expect(files.shown).toEqual({ kind: "file", file: 0 });
  files.nextMatch(1);
  expect([files.shown, files.jump]).toEqual([{ kind: "file", file: 1 }, { row: 24 }]);
});

it("shows a file by its path, as a NEWS entry asks, and nothing for a path the diff doesn't change", async () => {
  const files = await oneAtATime(changed());

  files.showPath("Python/generated.c.h");
  expect([files.shown, files.jump]).toEqual([{ kind: "file", file: 1 }, { page: { kind: "file", file: 1 } }]);
  files.jump = undefined;
  files.showPath("Lib/old.py");
  files.showFile(7);
  expect([files.shown, files.jump]).toEqual([{ kind: "file", file: 1 }, undefined]);
});

it("keeps the file on show when a newer diff comes, wherever that one lists it", async () => {
  const files = await oneAtATime(changed());
  files.showFile(1);

  core.pullRequestFiles.mockResolvedValue(
    ok(
      ready("def", 30, [file("Lib/new.py", 0), file("Lib/os.py", 5), file("Python/generated.c.h", 10, true)]),
    ),
  );
  await files.ask();

  expect(files.shown).toEqual({ kind: "file", file: 2 });
});

/** A file whose hunk has a line of context, a removed line and an added one, with rows 2 to 4. */
const picking: DiffRow[] = [
  { kind: "file", path: "a.py", old_path: null, change: { kind: "lines", added: 1, removed: 1 } },
  { kind: "hunk", text: "@@ -7,2 +7,2 @@" },
  { kind: "context", old: 7, new: 8, text: "def area(width):" },
  { kind: "removed", old: 8, text: "    return width * 2" },
  { kind: "added", new: 9, text: "    return width ** 2" },
];

/** The Files tab of `picking`'s diff at `head`, once its rows came. */
const pickable = async (head = "abc") => {
  core.pullRequestRows.mockResolvedValue(ok(picking));
  const files = await loaded(ready(head, picking.length, [file("a.py", 0)]));
  files.need(0, picking.length - 1);
  await vi.waitFor(() => {
    expect(files.row(4)).toBeDefined();
  });
  return files;
};

it("picks the word at a click, at the base for a removed line and at the head for the rest", async () => {
  const files = await pickable();
  const where = (row: number, offset: number) => {
    const pick = files.pickWord({ kind: "row", row }, null, offset, false);
    return pick && [pick.word, pick.side, pick.file, pick.line, pick.start, pick.end, pick.caret];
  };

  expect(where(2, 6)).toEqual(["area", "head", 0, 8, 4, 8, 6]);
  expect(where(3, 13)).toEqual(["width", "base", 0, 8, 11, 16, 13]);
  expect(where(4, 16)).toEqual(["width", "head", 0, 9, 11, 16, 16]);
  expect(files.picked?.word).toBe("width");
});

it("drops the pick on a click on no word, on a row that isn't code, and on a row that hasn't come", async () => {
  const files = await pickable();
  const pick = (row: number, offset: number) => {
    files.pickWord({ kind: "row", row: 2 }, null, 6, false);
    files.pickWord({ kind: "row", row }, null, offset, false);
    return files.picked;
  };

  expect(pick(4, 2)).toBeUndefined();
  expect(pick(1, 4)).toBeUndefined();
  expect(pick(0, 1)).toBeUndefined();
  expect(pick(9, 1)).toBeUndefined();
});

it("picks nothing before the diff comes", async () => {
  const files = await loaded();

  expect(files.pickWord({ kind: "row", row: 2 }, null, 6, false)).toBeUndefined();
});

it("selects the word on a double-click, with no caret drawn", async () => {
  const files = await pickable();

  files.pickWord({ kind: "row", row: 4 }, null, 12, true);

  expect(files.picked).toMatchObject({ word: "width", start: 11, end: 16, caret: null });
  expect(files.pickedIn({ kind: "row", row: 4 }, null)).toEqual({ start: 11, end: 16, caret: null });
});

it("shows the pick on its line alone, and on the half of a line of context it was made on", async () => {
  const files = await pickable();

  files.pickWord({ kind: "row", row: 2 }, "left", 5, false);

  expect(files.pickedIn({ kind: "row", row: 2 }, "left")).toEqual({ start: 4, end: 8, caret: 5 });
  expect(files.pickedIn({ kind: "row", row: 2 }, "right")).toBeUndefined();
  expect(files.pickedIn({ kind: "row", row: 2 }, null)).toEqual({ start: 4, end: 8, caret: 5 });
  expect(files.pickedIn({ kind: "row", row: 3 }, "left")).toBeUndefined();
  expect(files.pickedIn({ kind: "line", file: 0, line: 2 }, null)).toBeUndefined();
  files.pickWord({ kind: "row", row: 2 }, null, 5, false);
  expect(files.pickedIn({ kind: "row", row: 2 }, "right")).toEqual({ start: 4, end: 8, caret: 5 });
});

it("picks a word on a line an expanded gap shows, at the head", async () => {
  core.pullRequestLines.mockImplementation(({ start, end }) => Promise.resolve(ok(headLines(start, end))));
  const files = await loaded(gapped());
  files.expandAbove(small);
  await vi.waitFor(() => {
    expect(files.line(0, 3).text).toBe("line 3");
  });

  const pick = files.pickWord({ kind: "line", file: 0, line: 3 }, null, 2, false);

  expect(pick).toMatchObject({ word: "line", side: "head", file: 0, line: 3 });
  expect(files.pickedIn({ kind: "line", file: 0, line: 3 }, null)).toEqual({ start: 0, end: 4, caret: 2 });
  expect(files.pickWord({ kind: "line", file: 0, line: 40 }, null, 2, false)).toBeUndefined();
});

it("lets go of the pick when a newer diff comes, as its rows are other lines", async () => {
  const files = await pickable();
  files.pickWord({ kind: "row", row: 4 }, null, 12, false);

  core.pullRequestFiles.mockResolvedValue(ok(ready("def", picking.length, [file("a.py", 0)])));
  await files.ask();

  expect(files.picked).toBeUndefined();
});

/** An answer of a lookup with a hit in `path` at line `line` at the PR's base, or with none. */
const answerWith = (path?: string, line = 1): Answer => ({
  state: "found",
  hits:
    path === undefined
      ? []
      : [
          {
            place: { kind: "repository", path, side: "base", commit: "base" },
            line,
            column: 4,
            text: "def area(width):",
          },
        ],
});

/** The lookup of the word at `column` of line `line` of `path` at `side`, in `picking`'s diff at `head`. */
const lookupOf = (side: "base" | "head", path: string, line: number, column: number, head = "abc") => ({
  repository: "python/cpython",
  base: "base",
  head,
  path,
  side,
  line,
  column,
  // The window numbers its lookups across PRs, so the tests before this one took tickets too.
  ticket: expect.any(Number) as number,
});

/** The ticket of the `index`th lookup the test asked. */
const ticketOf = (index: number) => core.definitions.mock.calls[index]?.[0].ticket;

/** A lookup that answers only when the test answers it. */
const heldLookup = () => {
  let answer: (answer: unknown) => void = () => {};
  core.definitions.mockReturnValueOnce(
    new Promise((resolve) => {
      answer = resolve;
    }),
  );
  return (answered: unknown) => {
    answer(answered);
  };
};

it("looks up the picked word at the head, and at the base for a removed line", async () => {
  core.definitions.mockResolvedValue(ok(answerWith("Lib/shapes.py", 3)));
  const files = await pickable("abc");

  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await files.peekDefinition();
  files.pickWord({ kind: "row", row: 3 }, null, 13, false);
  await files.peekDefinition();

  expect(core.definitions.mock.calls.map(([lookup]) => lookup)).toEqual([
    lookupOf("head", "a.py", 8, 4),
    lookupOf("base", "a.py", 8, 11),
  ]);
  expect(files.peek).toMatchObject({
    at: { kind: "row", row: 3 },
    symbol: "width",
    side: "base",
    commit: "base",
    lookup: { state: "answered", answer: answerWith("Lib/shapes.py", 3) },
  });
});

it("looks up a removed line of a renamed file at its old path", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  core.pullRequestRows.mockResolvedValue(ok(picking));
  const files = await loaded(ready("abc", picking.length, [{ ...file("b.py", 0), old_path: "a.py" }]));
  files.need(0, picking.length - 1);
  await vi.waitFor(() => {
    expect(files.row(4)).toBeDefined();
  });

  files.pickWord({ kind: "row", row: 3 }, null, 13, false);
  await files.peekDefinition();
  files.pickWord({ kind: "row", row: 4 }, null, 13, false);
  await files.peekDefinition();

  expect(core.definitions.mock.calls.map(([lookup]) => lookup)).toEqual([
    lookupOf("base", "a.py", 8, 11),
    lookupOf("head", "b.py", 9, 11),
  ]);
});

it("looks up at the commit the diff shows while a newer head is fetched", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  core.pullRequestRows.mockResolvedValue(ok(picking));
  const before = ready("older", picking.length, [file("a.py", 0)]);
  const files = await loaded({ ...before, threads: { kind: "held_back", newer: "newer", count: 1 } });
  files.need(0, picking.length - 1);
  await vi.waitFor(() => {
    expect(files.row(4)).toBeDefined();
  });

  files.pickWord({ kind: "row", row: 4 }, null, 12, false);
  await files.peekDefinition();

  expect(core.definitions.mock.calls[0]?.[0]).toEqual(lookupOf("head", "a.py", 9, 11, "older"));
});

it("opens the peek under the picked line while the lookup runs, and pushes the rows after it down", async () => {
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const peekAt = (rows: number) => [
    { kind: "row", row: 2 },
    { kind: "thread", id: PEEK_ID, into: 0 },
    { kind: "thread", id: PEEK_ID, into: rows - 1 },
    { kind: "row", row: 3 },
  ];
  const placesAt = (rows: number) => [2, 3, 2 + rows, 3 + rows].map((place) => files.layout.at(place));

  const looking = files.peekDefinition();

  expect(files.peek?.lookup).toEqual({ state: "looking" });
  expect(placesAt(PEEK_ROWS)).toEqual(peekAt(PEEK_ROWS));
  answer(ok(answerWith("Lib/shapes.py", 3)));
  await looking;
  expect(files.peek?.lookup).toEqual({ state: "answered", answer: answerWith("Lib/shapes.py", 3) });
  expect(placesAt(PEEK_ROWS)).toEqual(peekAt(PEEK_ROWS));
});

it("shrinks the peek to its header and a line when it has no code to show", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.peekDefinition();

  expect([3, 4, 5].map((place) => files.layout.at(place))).toEqual([
    { kind: "thread", id: PEEK_ID, into: 0 },
    { kind: "thread", id: PEEK_ID, into: 1 },
    { kind: "row", row: 3 },
  ]);
});

it("opens no peek with no word picked", async () => {
  const files = await pickable();

  await files.peekDefinition();

  expect(files.peek).toBeUndefined();
  expect(core.definitions).not.toHaveBeenCalled();
});

it("closes the peek and hands the diff its focus, and a lookup that answers later opens nothing", async () => {
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const looking = files.peekDefinition();

  files.closePeek();
  answer(ok(answerWith("Lib/shapes.py", 3)));
  await looking;

  expect(files.peek).toBeUndefined();
  expect(files.diffWantsFocus).toBe(true);
  expect(files.layout.at(3)).toEqual({ kind: "row", row: 3 });
  // The lookup was still running, so the core cancels it.
  expect(core.cancelDefinitions.mock.calls).toEqual([[ticketOf(0)]]);
});

it("gives each lookup a ticket of its own, so a cancel names the lookup it means", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.peekDefinition();
  await files.peekDefinition();
  const again = await pickable();
  again.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await again.peekDefinition();

  const tickets = [0, 1, 2].map(ticketOf);
  expect(new Set(tickets).size).toBe(3);
});

it("cancels the lookup that waits when a newer diff closes its peek", async () => {
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const looking = files.peekDefinition();

  core.pullRequestFiles.mockResolvedValue(ok(ready("def", picking.length, [file("a.py", 0)])));
  await files.ask();
  answer(ok({ state: "cancelled" }));
  await looking;

  expect(files.peek).toBeUndefined();
  expect(core.cancelDefinitions.mock.calls).toEqual([[ticketOf(0)]]);
});

it("keeps the answer when a step of its lookup comes after it", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await files.peekDefinition();

  core.definitions.mock.calls[0]?.[1].onmessage({ step: "asking", server: "ty" });

  expect(files.peek?.lookup).toEqual({ state: "answered", answer: answerWith() });
});

it("cancels nothing when it closes a peek whose lookup has answered", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await files.peekDefinition();

  files.closePeek();

  expect(core.cancelDefinitions).not.toHaveBeenCalled();
});

it("says what the lookup waits for as the core says it, and when it started to", async () => {
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const clock = vi.spyOn(Date, "now").mockReturnValue(5000);
  const looking = files.peekDefinition();
  const waiting = core.definitions.mock.calls[0]?.[1];

  waiting?.onmessage({ step: "starting", server: "ty" });
  const starting = files.peek?.lookup;
  waiting?.onmessage({ step: "asking", server: "ty" });
  clock.mockRestore();

  expect(starting).toEqual({ state: "looking", waiting: { step: "starting", server: "ty" }, since: 5000 });
  expect(files.peek?.lookup).toEqual({
    state: "looking",
    waiting: { step: "asking", server: "ty" },
    since: 5000,
  });
  answer(ok(answerWith()));
  await looking;
  expect(files.peek?.lookup).toEqual({ state: "answered", answer: answerWith() });
});

it("leaves the peek alone when a lookup a newer one replaced says what it waits for", async () => {
  const older = heldLookup();
  core.definitions.mockResolvedValueOnce(ok(answerWith("Lib/b.py", 9)));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const first = files.peekDefinition();
  files.pickWord({ kind: "row", row: 4 }, null, 12, false);
  await files.peekDefinition();

  core.definitions.mock.calls[0]?.[1].onmessage({ step: "asking", server: "ty" });
  older(ok({ state: "cancelled" }));
  await first;

  expect(files.peek?.lookup).toEqual({ state: "answered", answer: answerWith("Lib/b.py", 9) });
});

it("shows the newest lookup when an older one answers after it", async () => {
  const older = heldLookup();
  core.definitions.mockResolvedValueOnce(ok(answerWith("Lib/b.py", 9)));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const first = files.peekDefinition();
  files.pickWord({ kind: "row", row: 4 }, null, 12, false);
  await files.peekDefinition();

  older(ok(answerWith("Lib/a.py", 1)));
  await first;

  expect(files.peek).toMatchObject({
    symbol: "width",
    lookup: { state: "answered", answer: answerWith("Lib/b.py", 9) },
  });
});

it("says why a lookup failed", async () => {
  const problem = "ty couldn't answer: the server ended before it answered";
  core.definitions.mockResolvedValue({ status: "error", error: { kind: "internal", message: problem } });
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.peekDefinition();

  expect(files.peek?.lookup).toEqual({ state: "failed", problem });
});

it("says why when the core can't be asked, and stops searching", async () => {
  core.definitions.mockRejectedValue(new Error("The window lost the core."));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.peekDefinition();

  expect(files.peek?.lookup).toEqual({ state: "failed", problem: "Error: The window lost the core." });
});

/** ty's hit at line `line` of `path` in the Repository, at `side`. */
const hitAt = (path: string, side: "base" | "head", line: number): Hit => ({
  place: { kind: "repository", path, side, commit: side },
  line,
  column: 4,
  text: "def area(width):",
});
const hitsFound = (...hits: Hit[]) => ok<Answer>({ state: "found", hits });
/** A line of PR 11's diff in `picking`, as Go to definition asks for its row. */
const lineOf = (path: string, side: "base" | "head", line: number): DiffLine => ({
  repository: "python/cpython",
  number: 11,
  head: "abc",
  base: "base",
  path,
  side,
  line,
});

it("goes to a hit on a line the diff shows, and marks the line on the hit's side", async () => {
  core.definitions.mockResolvedValue(hitsFound(hitAt("a.py", "head", 9)));
  core.pullRequestRow.mockResolvedValue(ok(4));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.goToDefinition();

  expect(core.definitions.mock.calls[0]?.[0]).toEqual(lookupOf("head", "a.py", 8, 4));
  expect(core.pullRequestRow.mock.calls).toEqual([[lineOf("a.py", "head", 9)]]);
  expect(files.jump).toEqual({ row: 4 });
  expect(files.marked).toEqual({ row: 4, side: "head" });
  expect(files.peek).toBeUndefined();
  expect(files.notice).toBeUndefined();
});

it("lands a hit at the base on the line's old side, and closes the peek", async () => {
  core.definitions.mockResolvedValueOnce(ok({ state: "not_set_up" }));
  core.definitions.mockResolvedValueOnce(hitsFound(hitAt("a.py", "base", 8)));
  core.pullRequestRow.mockResolvedValue(ok(3));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 3 }, null, 13, false);
  await files.peekDefinition();

  await files.goToDefinition();

  expect(core.pullRequestRow.mock.calls).toEqual([[lineOf("a.py", "base", 8)]]);
  expect(files.marked).toEqual({ row: 3, side: "base" });
  expect(files.peek).toBeUndefined();
});

it("says it searches while it looks, and for a moment that it found no definition", async () => {
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 3 }, null, 13, false);
  vi.useFakeTimers();

  const going = files.goToDefinition();
  const searching = files.notice;
  answer(hitsFound());
  await going;
  const nothing = files.notice;
  vi.advanceTimersByTime(NOTICE_MS);

  expect(searching).toBe("Searching for the definition of width…");
  expect(nothing).toBe("No definition of width at base.");
  expect(files.notice).toBeUndefined();
  expect(files.peek).toBeUndefined();
  expect(core.pullRequestRow).not.toHaveBeenCalled();
});

it("closes a peek on another word as it searches, and stops when the peek's Escape closes", async () => {
  core.definitions.mockResolvedValueOnce(ok({ state: "not_set_up" }));
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 3 }, null, 13, false);
  await files.peekDefinition();

  const going = files.goToDefinition();
  const peekWhileGoing = files.peek;
  const goingWhileSearching = files.going;
  files.closePeek();
  answer(hitsFound(hitAt("a.py", "head", 9)));
  await going;

  expect(peekWhileGoing).toBeUndefined();
  expect(goingWhileSearching).toBe(true);
  expect(files.going).toBe(false);
  expect(files.notice).toBeUndefined();
  expect(core.pullRequestRow).not.toHaveBeenCalled();
  expect(files.peek).toBeUndefined();
  // The search was still running, so the core cancels it, and only it.
  expect(core.cancelDefinitions.mock.calls).toEqual([[ticketOf(1)]]);
});

it("goes to the first hit the diff shows, past those outside it or whose row it can't find", async () => {
  const outside: Hit = {
    place: { kind: "outside", path: "C:/ty/builtins.pyi" },
    line: 1,
    column: 0,
    text: "",
  };
  const hits = ["gone.py", "error.py", "thrown.py", "a.py"].map((path) => hitAt(path, "head", 9));
  core.definitions.mockResolvedValue(hitsFound(outside, ...hits));
  core.pullRequestRow.mockResolvedValueOnce(ok(null));
  core.pullRequestRow.mockResolvedValueOnce({ status: "error", error: { kind: "internal", message: "no" } });
  core.pullRequestRow.mockRejectedValueOnce(new Error("The window lost the core."));
  core.pullRequestRow.mockResolvedValueOnce(ok(4));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  await files.goToDefinition();

  expect(core.pullRequestRow.mock.calls.map(([line]) => line.path)).toEqual([
    "gone.py",
    "error.py",
    "thrown.py",
    "a.py",
  ]);
  expect(files.marked).toEqual({ row: 4, side: "head" });
});

it("opens the peek on a hit on no line of the diff, or on an answer with no hit to go to", async () => {
  core.definitions.mockResolvedValueOnce(hitsFound(hitAt("Lib/shapes.py", "base", 3)));
  core.definitions.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "ty broke" },
  });
  core.pullRequestRow.mockResolvedValue(ok(null));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 3 }, null, 13, false);

  await files.goToDefinition();
  const nowhere = files.peek;
  await files.goToDefinition();

  expect(nowhere).toMatchObject({
    symbol: "width",
    lookup: { state: "answered", answer: { state: "found", hits: [hitAt("Lib/shapes.py", "base", 3)] } },
  });
  expect(nowhere?.code).toBeDefined();
  expect(files.peek?.lookup).toEqual({ state: "failed", problem: "ty broke" });
  expect(files.marked).toBeUndefined();
  expect(files.notice).toBeUndefined();
});

it("goes nowhere once a newer lookup started, or the peek closed, while it found the row", async () => {
  core.definitions.mockResolvedValue(hitsFound(hitAt("a.py", "head", 9)));
  let row: (row: unknown) => void = () => {};
  core.pullRequestRow.mockReturnValue(
    new Promise((resolve) => {
      row = resolve;
    }),
  );
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);

  const going = files.goToDefinition();
  await vi.waitFor(() => {
    expect(core.pullRequestRow).toHaveBeenCalled();
  });
  files.closePeek();
  row(ok(4));
  await going;

  expect(files.marked).toBeUndefined();
  expect(files.jump).toBeUndefined();
  expect(files.notice).toBeUndefined();
});

it("drops the search's words and the mark when a newer diff comes", async () => {
  core.definitions.mockResolvedValueOnce(hitsFound(hitAt("a.py", "head", 9)));
  core.pullRequestRow.mockResolvedValue(ok(4));
  const answer = heldLookup();
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await files.goToDefinition();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  const going = files.goToDefinition();

  core.pullRequestFiles.mockResolvedValue(ok(ready("def", picking.length, [file("a.py", 0)])));
  await files.ask();
  answer(hitsFound());
  await going;

  expect(files.marked).toBeUndefined();
  expect(files.notice).toBeUndefined();
});

it("closes the peek when a newer diff comes, as its rows are other lines", async () => {
  core.definitions.mockResolvedValue(ok(answerWith()));
  const files = await pickable();
  files.pickWord({ kind: "row", row: 2 }, null, 6, false);
  await files.peekDefinition();

  core.pullRequestFiles.mockResolvedValue(ok(ready("def", picking.length, [file("a.py", 0)])));
  await files.ask();

  expect(files.peek).toBeUndefined();
});
