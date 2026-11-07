import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import {
  CHUNK,
  PrFilesState,
  type DiffFile,
  type DiffRow,
  type NewsEntry,
  type PrFiles,
} from "$lib/state/pr-files.svelte";
import FilesTab from "./FilesTab.svelte";

const core = vi.hoisted(() => ({
  pullRequestFiles: vi.fn(),
  pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
  pullRequestSpans: vi.fn(),
  pullRequestLines: vi.fn<(lines: { file: number; start: number; end: number }) => Promise<unknown>>(),
  cloning: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequestFiles: core.pullRequestFiles,
    pullRequestRows: core.pullRequestRows,
    pullRequestSpans: core.pullRequestSpans,
    pullRequestLines: core.pullRequestLines,
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

const ready = (count: number, more: Partial<{ files: DiffFile[]; news: NewsEntry[] }> = {}): PrFiles => ({
  state: "ready",
  head: "abc",
  base: "base",
  rows: count,
  news: [],
  files: [
    {
      path: "news.txt",
      old_path: "notes.txt",
      change: { kind: "lines", added: 1, removed: 1 },
      row: 0,
      columns: 8,
      generated: false,
      gaps: [],
      rest: null,
    },
    {
      path: "logo.png",
      old_path: null,
      change: { kind: "binary" },
      row: 5,
      columns: 19,
      generated: false,
      gaps: [],
      rest: null,
    },
  ],
  ...more,
});

const onlink = vi.fn();

/** A HUD that times nothing, as these tests don't look at the jumps. */
const untimed = { jumped: () => ({ rowsShown: () => undefined, spansShown: () => undefined }) };

/** The Files tab of PR 11 once the core answered `answer`. */
const shown = async (answer: PrFiles | null) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, untimed);
  await files.load();
  render(FilesTab, { files, onlink });
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
  core.pullRequestSpans.mockReturnValue(new Promise(() => {}));
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
  const files = new PrFilesState("python/cpython", 11, untimed);
  await files.load();
  render(FilesTab, { files, onlink });

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
    base: "base",
    start: 0,
    end: CHUNK,
  });
  const diff = screen.getByRole("region", { name: "Diff" });
  const spacer = diff.querySelector(".spacer") as HTMLElement;
  expect(spacer.style.height).toBe(`${String(rows.length * 20)}px`);
  expect(spacer.style.minWidth).toBe("calc(19ch + 112px)");
});

it("draws rows near what shows, and asks for the chunks there as it scrolls", async () => {
  // The chunk after those that show waits for their spans.
  core.pullRequestSpans.mockResolvedValue(ok([]));
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

it("paints each line of code in its spans' colors once they come", async () => {
  const spans = [
    [],
    [],
    [{ start: 0, end: 5, highlight: "constant" }],
    [{ start: 4, end: 8, highlight: "keyword" }],
    [{ start: 0, end: 3, highlight: "function" }],
  ];
  core.pullRequestSpans.mockResolvedValue(ok(spans));

  await shown(ready(rows.length));

  await vi.waitFor(() => {
    expect(document.querySelector(".hl-keyword")?.textContent).toBe("line");
  });
  expect(document.querySelector(".hl-constant")?.textContent).toBe("Notes");
  expect(document.querySelector(".hl-function")?.textContent).toBe("new");
  // The text reads as it did before the spans came.
  expect(lines()[3]).toBe("2-old line");
});

it("starts a Generated file collapsed to its header, and opens it on a click", async () => {
  const file = (path: string, row: number, generated: boolean): DiffFile => ({
    path,
    old_path: null,
    change: { kind: "lines", added: 0, removed: 0 },
    row,
    columns: 8,
    generated,
    gaps: [],
    rest: null,
  });
  const files = [file("news.txt", 0, false), file("logo.png", 5, true), file("ast.py", 7, false)];
  await shown(ready(rows.length, { files }));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("notes.txt → news.txt+1 −1");
  });
  const toggle = screen.getByRole("button", { name: "▸logo.png" });
  const collapsed = [lines(), toggle.getAttribute("aria-expanded")];

  await fireEvent.click(toggle);

  expect(collapsed).toEqual([
    [
      "notes.txt → news.txt+1 −1",
      "@@ -1,2 +1,2 @@",
      "11 Notes",
      "2-old line",
      "2+new line",
      "▸logo.pngbinaryGenerated",
      "ast.py+0 −0",
    ],
    "false",
  ]);
  expect(lines().slice(5)).toEqual(["▾logo.pngbinaryGenerated", "Binary files differ", "ast.py+0 −0"]);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
});

it("renders the PR's NEWS entries above its rows, and follows their links in the browser", async () => {
  const news: NewsEntry[] = [
    {
      path: "Misc/NEWS.d/next/Library/2026-11-07.gh-issue-1.abc.rst",
      section: "Library",
      issue: { text: "gh-1", to: { site: "gh_issue", number: 1 } },
      blocks: [
        {
          kind: "paragraph",
          pieces: [
            { kind: "text", text: "Fix " },
            { kind: "code", text: "ast.parse()" },
            { kind: "text", text: " with " },
            { kind: "emphasis", text: "care" },
            { kind: "text", text: " and " },
            { kind: "strong", text: "force" },
            { kind: "text", text: "; see " },
            { kind: "link", text: "PEP 8", to: { site: "pep", number: 8 } },
            { kind: "text", text: "." },
          ],
        },
        { kind: "list", items: [[{ kind: "paragraph", pieces: [{ kind: "text", text: "One" }] }]] },
        { kind: "code", text: "x = 1" },
      ],
    },
    {
      path: "Misc/NEWS.d/next/C_API/notes.rst",
      section: "C API",
      issue: null,
      blocks: [{ kind: "paragraph", pieces: [{ kind: "text", text: "Changed." }] }],
    },
  ];
  await shown(ready(rows.length, { news }));

  const entries = screen.getByRole("region", { name: "NEWS entries" });
  const library = screen.getByRole("article", { name: "NEWS entry in Library" });
  await fireEvent.click(screen.getByRole("link", { name: "PEP 8" }));
  await fireEvent.click(screen.getByRole("link", { name: "gh-1" }));
  // A middle click follows a link too, and a right click or a click beside a link doesn't.
  await fireEvent(
    screen.getByRole("link", { name: "PEP 8" }),
    new MouseEvent("auxclick", { bubbles: true, button: 1 }),
  );
  await fireEvent(
    screen.getByRole("link", { name: "PEP 8" }),
    new MouseEvent("auxclick", { bubbles: true, button: 2 }),
  );
  await fireEvent.click(screen.getByText("Library"));

  // Each piece, as the tag that shows it and its text.
  const paragraph = library.querySelector("p:not(.about)");
  const pieces = [...(paragraph?.childNodes ?? [])]
    .filter((node) => node.nodeType !== Node.COMMENT_NODE && node.textContent !== "")
    .map((node) => [node.nodeName, node.textContent]);
  expect(pieces).toEqual([
    ["#text", "Fix "],
    ["CODE", "ast.parse()"],
    ["#text", " with "],
    ["EM", "care"],
    ["#text", " and "],
    ["STRONG", "force"],
    ["#text", "; see "],
    ["A", "PEP 8"],
    ["#text", "."],
  ]);
  expect(library.querySelector("li")?.textContent).toBe("One");
  expect(library.querySelector("pre")?.textContent).toBe("x = 1");
  const capi = screen.getByRole("article", { name: "NEWS entry in C API" });
  expect(capi.querySelector(".about")?.textContent.trim()).toBe("C API");
  expect(entries.lastElementChild).toBe(capi);
  expect(onlink.mock.calls).toEqual([
    ["https://peps.python.org/pep-0008/"],
    ["https://github.com/python/cpython/issues/1"],
    ["https://peps.python.org/pep-0008/"],
  ]);
});

it("shows no NEWS entries for a PR that has none", async () => {
  await shown(ready(rows.length));

  expect(screen.queryByRole("region", { name: "NEWS entries" })).toBeNull();
});

/** A file whose hunks leave out 49 lines above the first, 53 between the two, and maybe more after, which
 * has 150 lines at the head. */
const gapped: DiffRow[] = [
  { kind: "file", path: "a.py", old_path: null, change: { kind: "lines", added: 1, removed: 1 } },
  { kind: "hunk", text: "@@ -50,3 +50,3 @@" },
  { kind: "context", old: 50, new: 50, text: "a" },
  { kind: "removed", old: 51, text: "b" },
  { kind: "added", new: 51, text: "B" },
  { kind: "context", old: 52, new: 52, text: "c" },
  { kind: "hunk", text: "@@ -106,3 +106,3 @@" },
  { kind: "context", old: 106, new: 106, text: "d" },
  { kind: "context", old: 107, new: 107, text: "e" },
  { kind: "context", old: 108, new: 108, text: "f" },
];
const gappedFile: DiffFile = {
  path: "a.py",
  old_path: null,
  change: { kind: "lines", added: 1, removed: 1 },
  row: 0,
  columns: 8,
  generated: false,
  gaps: [
    { row: 1, old: 1, new: 1, lines: 49 },
    { row: 6, old: 53, new: 53, lines: 53 },
  ],
  rest: { old: 109, new: 109 },
};

/** The fake core sends `gapped`'s rows, and its lines with their first word a keyword. */
const setUpGapped = () => {
  core.pullRequestRows.mockResolvedValue(ok(gapped));
  core.pullRequestLines.mockImplementation(({ start, end }) =>
    Promise.resolve(
      ok(
        Array.from({ length: Math.min(end, 151) - start }, (_, at) => ({
          text: `def f${String(start + at)}`,
          spans: [{ start: 0, end: 3, highlight: "keyword" }],
        })),
      ),
    ),
  );
};

it("offers to show the lines each gap leaves out, and draws them highlighted once they come", async () => {
  setUpGapped();
  const files = await shown(ready(gapped.length, { files: [gappedFile] }));
  await vi.waitFor(() => {
    expect(lines()[1]).toBe("↑Whole file@@ -50,3 +50,3 @@");
  });
  const named = (name: string) => screen.getAllByRole("button", { name });
  const before = lines();

  await fireEvent.click(named("Show 20 more lines")[0] as HTMLElement);
  await vi.waitFor(() => {
    expect(lines()).toContain("109109 def f109");
  });
  await fireEvent.click(named("Show 20 lines below")[0] as HTMLElement);
  await vi.waitFor(() => {
    expect(lines()[6]).toBe("5353 def f53");
  });
  // The first gap's comes first.
  await fireEvent.click(named("Show 20 lines above")[1] as HTMLElement);
  await fireEvent.click((await screen.findByRole("button", { name: "Show 13 hidden lines" })) as HTMLElement);
  await vi.waitFor(() => {
    expect(files.layout.gapAt(6)).toBeUndefined();
  });
  await fireEvent.click(named("Show 20 lines above")[0] as HTMLElement);
  await fireEvent.click(named("Whole file")[0] as HTMLElement);
  await vi.waitFor(() => {
    expect(screen.queryByRole("button", { name: "Whole file" })).toBeNull();
  });

  expect(before.slice(6)).toEqual([
    "↑↓Whole file@@ -106,3 +106,3 @@",
    "106106 d",
    "107107 e",
    "108108 f",
    "↓Whole file",
  ]);
  expect(document.querySelector(".expanded .hl-keyword")?.textContent).toBe("def");
  expect(core.pullRequestLines.mock.calls.map(([lines]) => [lines.file, lines.start, lines.end])).toEqual([
    [0, 109, 129],
    [0, 53, 73],
    [0, 86, 106],
    [0, 73, 86],
    [0, 30, 50],
    [0, 1, 2 ** 32 - 1],
  ]);
});

it("shows the whole file from the row after its last hunk", async () => {
  setUpGapped();
  await shown(ready(gapped.length, { files: [gappedFile] }));
  await vi.waitFor(() => {
    expect(lines()[10]).toBe("↓Whole file");
  });

  await fireEvent.click(screen.getAllByRole("button", { name: "Whole file" }).at(-1) as HTMLElement);

  await vi.waitFor(() => {
    expect(screen.queryByRole("button", { name: "Whole file" })).toBeNull();
  });
  expect(core.pullRequestLines.mock.calls.map(([lines]) => [lines.start, lines.end])).toEqual([
    [1, 2 ** 32 - 1],
  ]);
});
