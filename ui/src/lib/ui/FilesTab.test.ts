import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { CHUNK, PrFilesState, type DiffRow, type PrFiles } from "$lib/state/pr-files.svelte";
import FilesTab from "./FilesTab.svelte";

const core = vi.hoisted(() => ({
  pullRequestFiles: vi.fn(),
  pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
  cloning: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequestFiles: core.pullRequestFiles,
    pullRequestRows: core.pullRequestRows,
    cloning: core.cloning,
  },
  events: {
    headsFetched: { listen: core.listen },
    changed: { listen: core.listen },
    cloningChanged: { listen: core.listen },
  },
}));

const ok = <T>(data: T) => ({ status: "ok" as const, data });

/** A diff of three files: a rename with an edit, a binary file, and a file that lost its last newline. */
const rows: DiffRow[] = [
  { kind: "file", path: "news.txt", old_path: "notes.txt", change: { kind: "lines", added: 1, removed: 1 } },
  { kind: "hunk", text: "@@ -1,2 +1,2 @@" },
  { kind: "context", old: 1, new: 1, text: "Notes" },
  { kind: "removed", old: 2, text: "old line" },
  { kind: "added", new: 2, text: "new line" },
  { kind: "file", path: "logo.png", old_path: null, change: { kind: "binary" } },
  { kind: "note", text: "Binary files differ" },
  { kind: "file", path: "ast.py", old_path: null, change: { kind: "lines", added: 0, removed: 0 } },
];

const ready = (count: number): PrFiles => ({
  state: "ready",
  head: "abc",
  files: [
    {
      path: "news.txt",
      old_path: "notes.txt",
      change: { kind: "lines", added: 1, removed: 1 },
      row: 0,
      columns: 8,
    },
    { path: "logo.png", old_path: null, change: { kind: "binary" }, row: 5, columns: 19 },
  ],
  rows: count,
});

/** The Files tab of PR 11 once the core answered `answer`. */
const shown = async (answer: PrFiles | null) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11);
  await files.load();
  render(FilesTab, { files });
  return files;
};

const lines = () => [...document.querySelectorAll(".row")].map((row) => row.textContent);

beforeEach(() => {
  vi.resetAllMocks();
  // jsdom lays nothing out, so bind:clientHeight has nothing to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  core.listen.mockResolvedValue(vi.fn());
  core.cloning.mockResolvedValue(null);
  core.pullRequestRows.mockResolvedValue(ok(rows));
});

it("waits for the mirror's clone, and says how far it got", async () => {
  core.cloning.mockResolvedValue({
    repository: "python/cpython",
    progress: { stage: "receiving", percent: 40 },
  });
  await shown({ state: "cloning" });
  expect(screen.getByText("Cloning python/cpython 40%")).toBeTruthy();
});

it("waits for the mirror's clone before it hears how far it got", async () => {
  await shown({ state: "cloning" });
  expect(screen.getByText("Cloning the Repository's mirror…")).toBeTruthy();
});

it("waits for the PR's commits", async () => {
  await shown({ state: "fetching" });
  expect(screen.getByText("Fetching the PR's commits…")).toBeTruthy();
});

it("shows nothing for a PR Tsugiki doesn't hold", async () => {
  await shown(null);
  expect(document.body.textContent.trim()).toBe("");
});

it("says why it can't show the files", async () => {
  core.pullRequestFiles.mockResolvedValue({ status: "error", error: { message: "The mirror has no main" } });
  const files = new PrFilesState("python/cpython", 11);
  await files.load();
  render(FilesTab, { files });

  expect(screen.getByRole("alert").textContent).toBe(
    "Tsugiki can't show this PR's files: The mirror has no main",
  );
});

it("draws each kind of row once the core sends them", async () => {
  await shown(ready(rows.length));

  await vi.waitFor(() => {
    expect(lines()[0]).toBe("notes.txt → news.txt+1 −1");
  });

  expect(lines()).toEqual([
    "notes.txt → news.txt+1 −1",
    "@@ -1,2 +1,2 @@",
    "11 Notes",
    "2-old line",
    "2+new line",
    "logo.pngbinary",
    "Binary files differ",
    "ast.py+0 −0",
  ]);
  expect(core.pullRequestRows).toHaveBeenCalledExactlyOnceWith({
    repository: "python/cpython",
    number: 11,
    head: "abc",
    start: 0,
    end: CHUNK,
  });
  const diff = screen.getByRole("region", { name: "Diff" });
  const spacer = diff.firstElementChild as HTMLElement;
  expect(spacer.style.height).toBe(`${String(rows.length * 20)}px`);
  expect(spacer.style.minWidth).toBe("calc(19ch + 112px)");
});

it("draws rows near what shows, and asks for the chunks there as it scrolls", async () => {
  await shown(ready(10 * CHUNK));
  // Nothing is visible in a test, so it draws only the rows above and below.
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("notes.txt → news.txt+1 −1");
  });
  expect(lines()).toHaveLength(41);
  expect(lines()[40]).toBe("");

  const diff = screen.getByRole("region", { name: "Diff" });
  diff.scrollTop = 4 * CHUNK * 20;
  await fireEvent.scroll(diff);

  await vi.waitFor(() => {
    expect(core.pullRequestRows).toHaveBeenCalledTimes(5);
  });
  const starts = core.pullRequestRows.mock.calls.map(([chunk]) => chunk.start / CHUNK);
  expect(starts).toEqual([0, 1, 3, 4, 5]);
  expect(lines()).toHaveLength(81);
});
