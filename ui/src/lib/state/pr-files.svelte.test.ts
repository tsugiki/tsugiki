import { beforeEach, expect, it, vi } from "vitest";
import { CHUNK, PrFilesState, type DiffRow, type PrFiles } from "./pr-files.svelte";

type Heard = (event: { payload: unknown }) => void;

const core = vi.hoisted(() => {
  const core = {
    pullRequestFiles: vi.fn(),
    pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
    pullRequestSpans: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
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
    cloning: core.cloning,
  },
  events: {
    headsFetched: core.listener("headsFetched"),
    changed: core.listener("changed"),
    cloningChanged: core.listener("cloningChanged"),
  },
}));

const ready = (head: string, rows: number): PrFiles => ({
  state: "ready",
  head,
  files: [
    { path: "NEWS", old_path: null, change: { kind: "lines", added: 1, removed: 0 }, row: 0, columns: 4 },
  ],
  rows,
});

const ok = <T>(data: T) => ({ status: "ok" as const, data });
const added = (text: string): DiffRow => ({ kind: "added", new: 1, text });
/** The spans the fake core sends for the first row of the chunk at `start`. */
const keyword = (start: number) => [{ start: 0, end: start + 1, highlight: "keyword" as const }];
const cloning = { repository: "python/cpython", progress: { stage: "receiving", percent: 40 } } as const;

/** A Files tab for PR 11 that has loaded, with the core answering `answer`. */
const loaded = async (answer: PrFiles | null = { state: "fetching" }) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11);
  await files.load();
  return files;
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
});

it("asks for each chunk's spans with its rows, and shows the rows before the spans come", async () => {
  const files = await loaded(ready("abc", 10));
  let answerSpans: (answer: unknown) => void = () => {};
  core.pullRequestSpans.mockReturnValueOnce(
    new Promise((resolve) => {
      answerSpans = resolve;
    }),
  );

  files.need(0, 9);
  await vi.waitFor(() => {
    expect(files.row(0)).toEqual(added("row 0"));
  });
  expect(files.spans(0)).toBeUndefined();
  answerSpans(ok([keyword(0)]));

  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(0));
  });
  expect(files.spans(1)).toBeUndefined();
  expect(core.pullRequestSpans).toHaveBeenCalledExactlyOnceWith({
    repository: "python/cpython",
    number: 11,
    head: "abc",
    start: 0,
    end: CHUNK,
  });
});

it("lets go of a chunk's spans with its rows, and drops those that come once it's gone", async () => {
  const files = await loaded(ready("abc", 20 * CHUNK));
  files.need(0, 10);
  await vi.waitFor(() => {
    expect(files.spans(0)).toEqual(keyword(0));
  });
  let answerLate: (answer: unknown) => void = () => {};
  core.pullRequestSpans.mockReturnValueOnce(
    new Promise((resolve) => {
      answerLate = resolve;
    }),
  );
  files.need(5 * CHUNK, 5 * CHUNK + 10);
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
    [{ repository: "python/cpython", number: 11, head: "abc", start: 0, end: CHUNK }],
    [{ repository: "python/cpython", number: 11, head: "abc", start: CHUNK, end: 2 * CHUNK }],
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
  expect(core.pullRequestRows).toHaveBeenCalledTimes(9);
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
