import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  CHUNK,
  DiffViewChoice,
  EXPAND,
  PrFilesState,
  SPANS_WAIT,
  type DiffFile,
  type DiffRow,
  type PrFiles,
} from "./pr-files.svelte";

type Heard = (event: { payload: unknown }) => void;

const core = vi.hoisted(() => {
  const core = {
    pullRequestFiles: vi.fn(),
    pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
    pullRequestSpans: vi.fn<(chunk: { start: number }, opened?: number[]) => Promise<unknown>>(),
    pullRequestLines: vi.fn<(lines: { file: number; start: number; end: number }) => Promise<unknown>>(),
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
    cloning: core.cloning,
  },
  events: {
    headsFetched: core.listener("headsFetched"),
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
const ready = (head: string, rows: number, files = [file("NEWS", 0)], base = "base"): PrFiles => ({
  state: "ready",
  head,
  base,
  files,
  rows,
  news: [],
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
  expect(core.stopped.toSorted()).toEqual(["changed", "cloningChanged", "headsFetched"]);

  core.stopped = [];
  const early = new PrFilesState("python/cpython", 11);
  const loading = early.load();
  early.stop();
  await loading;

  expect(core.stopped).toHaveLength(3);
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
  { row: 1, old: 1, new: 1, lines: 5 },
  { row: 6, old: 10, new: 12, lines: 45 },
];

/** Where each ask for lines started and ended. */
const asked = () => core.pullRequestLines.mock.calls.map(([lines]) => [lines.start, lines.end]);

it("shows 20 more lines above a hunk or below the one before, and then the rest of a short gap", async () => {
  const files = await loaded(gapped());

  files.expandAbove(0, large);
  await vi.waitFor(() => {
    expect(files.line(0, 56)).toEqual({ text: "line 56", spans: [] });
  });
  files.expandBelow(0, large);
  await vi.waitFor(() => {
    expect(files.layout.gapAt(6)?.hidden).toBe(45 - 2 * EXPAND);
  });
  files.expandGap(0, large);
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

  files.expandBelow(0, small);
  await vi.waitFor(() => {
    expect(files.layout.gapAt(1)).toBeUndefined();
  });
  files.expandAbove(0, small);
  files.expandGap(0, small);

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
  files.expandAbove(0, large);
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
  expect(files.layout.gapAt(6)?.hidden).toBe(45);
  expect(files.layout.at(20)).toEqual({ kind: "more", file: 0 });
});

it("keeps the button when the core can't send lines, and says why", async () => {
  const files = await loaded(gapped());
  const failed = { status: "error", error: { message: "gone" } };
  core.pullRequestLines.mockResolvedValue(failed);

  files.expandAbove(0, large);
  await files.expandRest(0);
  await files.expandFile(0);

  expect(files.error).toBe("gone");
  expect(files.layout.at(20)).toEqual({ kind: "more", file: 0 });
  expect(files.layout.gapAt(6)?.hidden).toBe(45);
});

it("expands nothing until the diff is ready, nor after a file that ends with its last hunk", async () => {
  const files = await loaded();

  files.expandAbove(0, large);
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
