import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  CHUNK,
  DiffViewChoice,
  EXPAND,
  PrFilesState,
  SPANS_WAIT,
  type DiffFile,
  type DiffRow,
  type DiffThread,
  type PrFiles,
} from "./pr-files.svelte";

type Heard = (event: { payload: unknown }) => void;

const core = vi.hoisted(() => {
  const core = {
    pullRequestFiles: vi.fn(),
    pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
    pullRequestSpans: vi.fn<(chunk: { start: number }, opened?: number[]) => Promise<unknown>>(),
    pullRequestLines: vi.fn<(lines: { file: number; start: number; end: number }) => Promise<unknown>>(),
    pullRequestFind: vi.fn<(search: { query: string }) => Promise<unknown>>(),
    cloning: vi.fn(),
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
    cloning: core.cloning,
  },
  events: {
    headsFetched: core.listener("headsFetched"),
    headsFetchFailed: core.listener("headsFetchFailed"),
    changed: core.listener("changed"),
    cloningChanged: core.listener("cloningChanged"),
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
  expect(core.stopped.toSorted()).toEqual(["changed", "cloningChanged", "headsFetchFailed", "headsFetched"]);

  core.stopped = [];
  const early = new PrFilesState("python/cpython", 11);
  const loading = early.load();
  early.stop();
  await loading;

  expect(core.stopped).toHaveLength(4);
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

it("asks again for the spans of the chunks it holds once a Generated file opens, naming it", async () => {
  // Spans that name the files the window has opened, so the test can tell an answer to a later ask.
  core.pullRequestSpans.mockImplementation(({ start }, opened = []) =>
    Promise.resolve(ok([keyword(start + opened.length)])),
  );
  const files = await loaded(withGenerated("abc"));
  files.need(0, 20);
  await vi.waitFor(() => {
    expect(files.spans(2 * CHUNK)).toEqual(keyword(2 * CHUNK));
  });

  files.toggle(1);
  files.need(0, 20);

  await vi.waitFor(() => {
    expect(core.pullRequestSpans).toHaveBeenCalledTimes(5);
  });
  const asked = core.pullRequestSpans.mock.calls.map(([chunk, opened]) => [chunk.start, opened]);
  expect(asked).toEqual([
    [0, []],
    [2 * CHUNK, []],
    [0, [1]],
    [2 * CHUNK, [1]],
    [CHUNK, [1]],
  ]);
  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(1));
  });
});

it("keeps a chunk's spans when asking again fails, and drops those that come once it's gone", async () => {
  const files = await loaded(withGenerated("abc"));
  files.need(0, 5);
  await vi.waitFor(() => {
    expect(files.spans(2 * CHUNK)).toEqual(keyword(2 * CHUNK));
  });
  core.pullRequestSpans.mockResolvedValueOnce({ status: "error", error: { message: "No tree" } });
  const answerLate = heldSpans();

  files.toggle(1);
  core.pullRequestFiles.mockResolvedValue(ok(withGenerated("def")));
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
