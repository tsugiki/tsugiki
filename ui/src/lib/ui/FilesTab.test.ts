import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { chooseMirrorSource, mirrorCost, pickClone } from "$lib/state/mirror-source";
import {
  CHUNK,
  DiffViewChoice,
  PrFilesState,
  type DiffFile,
  type DiffRow,
  type DiffThread,
  type NewsEntry,
  type PrFiles,
} from "$lib/state/pr-files.svelte";
import type { YourReview } from "$lib/state/workbench.svelte";
import FilesTab from "./FilesTab.svelte";

const core = vi.hoisted(() => ({
  pullRequestFiles: vi.fn(),
  pullRequestRows: vi.fn<(chunk: { start: number }) => Promise<unknown>>(),
  pullRequestSpans: vi.fn(),
  pullRequestLines: vi.fn<(lines: { file: number; start: number; end: number }) => Promise<unknown>>(),
  pullRequestFind: vi.fn(),
  definitions: vi.fn(),
  definitionCode: vi.fn<(code: { start: number; end: number }) => Promise<unknown>>(),
  cloning: vi.fn(),
  mirrorStates: vi.fn(),
  mirrorsHeld: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("$lib/data/bindings", () => ({
  commands: {
    pullRequestFiles: core.pullRequestFiles,
    pullRequestRows: core.pullRequestRows,
    pullRequestSpans: core.pullRequestSpans,
    pullRequestLines: core.pullRequestLines,
    pullRequestFind: core.pullRequestFind,
    definitions: core.definitions,
    definitionCode: core.definitionCode,
    cloning: core.cloning,
    mirrorStates: core.mirrorStates,
    mirrorsHeld: core.mirrorsHeld,
  },
  events: {
    headsFetched: { listen: core.listen },
    headsFetchFailed: { listen: core.listen },
    changed: { listen: core.listen },
    readFailed: { listen: core.listen },
    cloningChanged: { listen: core.listen },
    mirrorStatesChanged: { listen: core.listen },
    mirrorsHeldChanged: { listen: core.listen },
  },
}));

vi.mock("$lib/state/mirror-source", () => ({
  pickClone: vi.fn(),
  chooseMirrorSource: vi.fn(),
  mirrorCost: vi.fn(),
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

const ready = (
  count: number,
  more: Partial<{ files: DiffFile[]; news: NewsEntry[]; threads: DiffThread[]; waiting: number }> = {},
): PrFiles => ({
  state: "ready",
  head: "abc",
  base: "base",
  rows: count,
  news: more.news ?? [],
  threads:
    more.waiting === undefined
      ? { kind: "shown", threads: more.threads ?? [] }
      : { kind: "held_back", newer: "def5678def", count: more.waiting },
  files: more.files ?? [
    {
      path: "news.txt",
      old_path: "notes.txt",
      change: { kind: "lines", added: 1, removed: 1 },
      row: 0,
      columns: 8,
      generated: false,
      gaps: [],
      rest: null,
      pairs: [],
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
      pairs: [],
    },
  ],
});

const onlink = vi.fn();
const NOW = new Date("2026-11-04T12:00:00Z");

/** A HUD that times nothing, as these tests don't look at the jumps. */
const untimed = { jumped: () => ({ rowsShown: () => undefined, spansShown: () => undefined }) };

/** The Files tab of PR 11 once the core answered `answer`, in the view `choice` holds, with the user's
 * Review `review`. */
const shown = async (answer: PrFiles | null, choice = new DiffViewChoice(), review?: YourReview) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, untimed, choice);
  files.followReview(review);
  await files.load();
  render(FilesTab, { files, now: NOW, onlink });
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
  core.mirrorStates.mockResolvedValue([]);
  core.mirrorsHeld.mockResolvedValue(false);
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

describe("the question of where the mirror gets its objects", () => {
  const lender = String.raw`D:\cpython`;
  const cost = { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 };
  const start = () => screen.getByRole("button", { name: "Start the clone" });

  beforeEach(() => {
    vi.mocked(mirrorCost).mockResolvedValue(cost);
  });

  it("asks it instead of saying it is cloning, with the time and disk of each way", async () => {
    await shown({ state: "asking" });

    expect(screen.getByText(/its own copy of python\/cpython, its mirror, which waits for you/)).toBeTruthy();
    expect(screen.queryByText(/Cloning/)).toBeNull();
    expect(mirrorCost).toHaveBeenCalledWith("python/cpython");
    expect(await screen.findByText("Mirror, for the diff: 891 MB on the disk either way.")).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Download it from GitHub, about 3 minutes" })).toBeTruthy();
    expect(
      screen.getByRole("radio", { name: "Borrow objects from a clone you have, about 1 minute" }),
    ).toBeTruthy();
  });

  it("offers both ways without their cost when GitHub can't say it", async () => {
    vi.mocked(mirrorCost).mockResolvedValue(null);
    await shown({ state: "asking" });

    expect(await screen.findByText("Mirror, for the diff")).toBeTruthy();
    expect(screen.getByRole("radio", { name: "Download it from GitHub" })).toBeTruthy();
  });

  it("starts the clone with Download", async () => {
    await shown({ state: "asking" });

    await fireEvent.click(start());

    expect(chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "download" });
    await vi.waitFor(() => {
      expect((start() as HTMLButtonElement).disabled).toBe(true);
    });
  });

  it("says why a folder can't lend, and starts nothing until the user picks another or Download", async () => {
    vi.mocked(pickClone).mockResolvedValueOnce({
      kind: "refused",
      folder: lender,
      refusal: { kind: "no_remote" },
    });
    await shown({ state: "asking" });

    await fireEvent.click(screen.getByRole("radio", { name: /^Borrow objects/ }));

    expect(await screen.findByText(String.raw`D:\cpython has no remote for python/cpython.`)).toBeTruthy();
    expect((start() as HTMLButtonElement).disabled).toBe(true);
    vi.mocked(pickClone).mockResolvedValueOnce({ kind: "lends", folder: lender });
    await fireEvent.click(screen.getByRole("button", { name: "Choose another clone…" }));
    await vi.waitFor(() => {
      expect((start() as HTMLButtonElement).disabled).toBe(false);
    });
    await fireEvent.click(start());
    expect(chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "borrow", folder: lender });
  });

  it("says why the answer didn't take", async () => {
    vi.mocked(chooseMirrorSource).mockResolvedValue("machine.toml, line 1: expected a value");
    await shown({ state: "asking" });

    await fireEvent.click(start());

    expect((await screen.findByRole("alert")).textContent).toBe("machine.toml, line 1: expected a value");
  });
});

it("waits for the PR's commits", async () => {
  await shown({ state: "fetching" });
  expect(screen.getByText("Fetching the PR's commits…")).toBeTruthy();
});

it("says the PR's commits wait for the network while the mirror holds back git, over a failed fetch", async () => {
  core.mirrorsHeld.mockResolvedValue(true);
  const files = await shown({ state: "fetching" });
  expect(screen.getByRole("status").textContent).toBe("The PR's commits come once GitHub answers.");

  files.unfetched = { kind: "git", words: "fatal: unable to access" };
  await tick();
  expect(screen.getByRole("status").textContent).toBe("The PR's commits come once GitHub answers.");
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
});

it("says why the PR's commits didn't come while the mirror holds back git, when the network doesn't explain it", async () => {
  core.mirrorsHeld.mockResolvedValue(true);
  const files = await shown(ready(rows.length, { waiting: 2 }));
  files.unfetched = { kind: "not_on_github" };
  await tick();

  expect(screen.getByRole("alert").textContent).toBe(
    "Showing abc · GitHub has no copy of this PR's branch. 2 threads wait for it. Try again",
  );
});

it("says the newer head waits for the network over the diff before, with its threads counted", async () => {
  core.mirrorsHeld.mockResolvedValue(true);
  const files = await shown(ready(rows.length, { waiting: 2 }));
  files.unfetched = { kind: "git", words: "" };
  await tick();

  expect(screen.getByRole("status").textContent).toBe(
    "Showing abc · the newer def5678 comes once GitHub answers. 2 threads wait for it.",
  );
  expect(screen.queryByRole("alert")).toBeNull();
});

it("counts no threads beside a newer head that waits for the network when none wait", async () => {
  core.mirrorsHeld.mockResolvedValue(true);
  await shown(ready(rows.length, { waiting: 0 }));

  expect(screen.getByRole("status").textContent).toBe(
    "Showing abc · the newer def5678 comes once GitHub answers. ",
  );
});

it("says the mirror's clone waits for the network", async () => {
  core.mirrorStates.mockResolvedValue([{ repository: "python/cpython", state: { state: "offline" } }]);
  await shown({ state: "cloning" });
  expect(screen.getByText("Cloning python/cpython starts once GitHub answers.")).toBeTruthy();
});

it("shows nothing for a PR Tsugiki doesn't hold", async () => {
  await shown(null);
  expect(document.body.textContent.trim()).toBe("");
});

it("says why it can't show the files", async () => {
  core.pullRequestFiles.mockResolvedValue({ status: "error", error: { message: "The mirror has no main" } });
  const files = new PrFilesState("python/cpython", 11, untimed);
  await files.load();
  render(FilesTab, { files, now: NOW, onlink });

  expect(screen.getByRole("alert").textContent).toBe(
    "Tsugiki can't show this PR's files: The mirror has no main",
  );
});

it("draws each kind of row once the core sends them", async () => {
  await shown(ready(rows.length));

  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });

  expect(lines()).toEqual([
    "▾notes.txt → news.txt+1 −1",
    "@@ -1,2 +1,2 @@",
    "11 Notes",
    "2-old line",
    "2+new line",
    "▾logo.pngbinary",
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
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
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

it("starts a Generated file collapsed to its header, and collapses or opens any file on a click", async () => {
  const file = (path: string, row: number, generated: boolean): DiffFile => ({
    path,
    old_path: null,
    change: { kind: "lines", added: 0, removed: 0 },
    row,
    columns: 8,
    generated,
    gaps: [],
    rest: null,
    pairs: [],
  });
  const files = [file("news.txt", 0, false), file("logo.png", 5, true), file("ast.py", 7, false)];
  await shown(ready(rows.length, { files }));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });
  const toggle = screen.getByRole("button", { name: "▸logo.png" });
  const collapsed = [lines(), toggle.getAttribute("aria-expanded")];

  await fireEvent.click(toggle);

  expect(collapsed).toEqual([
    [
      "▾notes.txt → news.txt+1 −1",
      "@@ -1,2 +1,2 @@",
      "11 Notes",
      "2-old line",
      "2+new line",
      "▸logo.pngbinaryGenerated",
      "▾ast.py+0 −0",
    ],
    "false",
  ]);
  expect(lines().slice(5)).toEqual(["▾logo.pngbinaryGenerated", "Binary files differ", "▾ast.py+0 −0"]);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  const first = screen.getByRole("button", { name: "▾notes.txt → news.txt" });
  await fireEvent.click(first);
  expect(lines()).toEqual([
    "▸notes.txt → news.txt+1 −1",
    "▾logo.pngbinaryGenerated",
    "Binary files differ",
    "▾ast.py+0 −0",
  ]);
  expect(first.getAttribute("aria-expanded")).toBe("false");
});

it("says in their headers which files changed since a Stale review, and starts the rest folded", async () => {
  const file = (path: string, row: number, old_path: string | null = null): DiffFile => ({
    path,
    old_path,
    change: { kind: "lines", added: 0, removed: 0 },
    row,
    columns: 8,
    generated: false,
    gaps: [],
    rest: null,
    pairs: [],
  });
  const files = [file("news.txt", 0, "notes.txt"), file("logo.png", 5), file("ast.py", 7)];
  const review: YourReview = {
    kind: "stale_review",
    reviewed: "1".repeat(40),
    head: "abc",
    files: [
      { path: "news.txt", whitespace_only: false },
      { path: "ast.py", whitespace_only: true },
    ],
  };
  await shown(ready(rows.length, { files }), new DiffViewChoice(), review);

  await vi.waitFor(() => {
    expect(lines()).toEqual([
      "▾notes.txt → news.txt+1 −1Changed since your Review",
      "@@ -1,2 +1,2 @@",
      "11 Notes",
      "2-old line",
      "2+new line",
      "▸logo.pngbinary",
      "▾ast.py+0 −0Whitespace only",
    ]);
  });
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
  expect(capi.querySelector(".about")?.textContent.replace(/\s+/g, " ").trim()).toBe("C API Show file");
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
  pairs: [],
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

it("opens the lines around a thread on a line no hunk shows, with buttons for those hidden around them", async () => {
  setUpGapped();
  const threads = [
    threadAt("T1", { kind: "outside", file: 0, line: 80, head_line: 80 }, "open"),
    threadAt("T2", { kind: "gone", path: "Lib/old.py", code: null }, "open"),
  ];
  await shown(ready(gapped.length, { files: [gappedFile], threads }));
  await vi.waitFor(() => {
    expect(lines()).toContain("8080 def f80");
  });

  // Lines 53 to 76 hide above the window, and 84 to 105 below it, under the hunk's row.
  const above = lines().indexOf("7777 def f77");
  expect(lines()[above - 1]).toBe("↑↓Whole file");
  expect(lines().at(-1)).toBe("On files no longer in the diff");
  expect(screen.getAllByRole("group", { name: "Review thread" })).toHaveLength(2);
  await fireEvent.click(screen.getAllByRole("button", { name: "Show 20 lines below" })[0] as HTMLElement);
  await vi.waitFor(() => {
    expect(lines()).toContain("5353 def f53");
  });
  expect(core.pullRequestLines.mock.calls.map(([lines]) => [lines.start, lines.end])).toEqual([
    [77, 84],
    [53, 73],
  ]);
});

it("shows the diff before a moved head under a line that says so, with its threads counted", async () => {
  await shown(ready(rows.length, { waiting: 2 }));

  expect(screen.getByRole("status").textContent).toBe(
    "Showing abc · fetching the newer def5678. 2 threads wait for it.",
  );
});

it("says one thread waits for the newer diff, and counts none when there are none", async () => {
  await shown(ready(rows.length, { waiting: 1 }));
  await shown(ready(rows.length, { waiting: 0 }));

  expect(screen.getAllByRole("status").map((status) => status.textContent)).toEqual([
    "Showing abc · fetching the newer def5678. 1 thread waits for it.",
    "Showing abc · fetching the newer def5678. ",
  ]);
});

it("says why the newer head failed to come over the diff before, and fetches it again on Try again", async () => {
  const files = await shown(ready(rows.length, { waiting: 2 }));
  files.unfetched = {
    kind: "git",
    words: "fatal: unable to access 'https://github.com/python/cpython.git/'",
  };
  await tick();

  expect(screen.getByRole("alert").textContent).toBe(
    "Showing abc · Tsugiki couldn't fetch the newer def5678 " +
      "(fatal: unable to access 'https://github.com/python/cpython.git/'). 2 threads wait for it. Try again",
  );
  await vi.waitFor(() => {
    expect(lines()).toContain("2+new line");
  });
  await fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(core.pullRequestFiles).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("status").textContent).toBe(
    "Showing abc · fetching the newer def5678. 2 threads wait for it.",
  );
});

it("counts no threads beside a failed fetch of the newer head when none wait", async () => {
  const files = await shown(ready(rows.length, { waiting: 0 }));
  files.unfetched = { kind: "git", words: "" };
  await tick();

  expect(screen.getByRole("alert").textContent).toBe(
    "Showing abc · Tsugiki couldn't fetch the newer def5678.  Try again",
  );
});

it("says GitHub's copy of the branch isn't at the newer head, or isn't there, over the diff before", async () => {
  const files = await shown(ready(rows.length, { waiting: 2 }));
  const alerts: (string | null)[] = [];
  for (const why of [{ kind: "behind" }, { kind: "not_on_github" }] as const) {
    files.unfetched = why;
    await tick();
    alerts.push(screen.getByRole("alert").textContent);
  }

  expect(alerts).toEqual([
    "Showing abc · GitHub's copy of this PR's branch isn't at def5678. 2 threads wait for it. Try again",
    "Showing abc · GitHub has no copy of this PR's branch. 2 threads wait for it. Try again",
  ]);
});

it("says why the PR's commits failed to come, and fetches them again on Try again", async () => {
  const files = await shown({ state: "fetching" });
  files.unfetched = { kind: "git", words: "" };
  await tick();

  expect(screen.getByRole("alert").textContent).toBe("Tsugiki couldn't fetch the PR's commits. Try again");
  await fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(screen.getByText("Fetching the PR's commits…")).toBeTruthy();
});

it("says GitHub's copy of the branch isn't at the PR's head, or isn't there, in place of fetching", async () => {
  const files = await shown({ state: "fetching" });
  const alerts: (string | null)[] = [];
  for (const why of [{ kind: "behind" }, { kind: "not_on_github" }] as const) {
    files.unfetched = why;
    await tick();
    alerts.push(screen.getByRole("alert").textContent);
  }

  expect(alerts).toEqual([
    "GitHub's copy of this PR's branch isn't at the commit Tsugiki last synced. Try again",
    "GitHub has no copy of this PR's branch. Try again",
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

/** A file that replaces 2 lines with 1, the second without a newline, removes a line, and adds one. */
const changed: DiffRow[] = [
  { kind: "file", path: "a.py", old_path: null, change: { kind: "lines", added: 2, removed: 3 } },
  { kind: "hunk", text: "@@ -1,6 +1,5 @@" },
  { kind: "context", old: 1, new: 1, text: "a" },
  { kind: "removed", old: 2, text: "b" },
  { kind: "removed", old: 3, text: "c" },
  { kind: "note", text: "No newline at end of file" },
  { kind: "added", new: 2, text: "B" },
  { kind: "context", old: 4, new: 3, text: "d" },
  { kind: "removed", old: 5, text: "e" },
  { kind: "context", old: 6, new: 4, text: "f" },
  { kind: "added", new: 5, text: "g" },
];
const changedFile: DiffFile = {
  ...gappedFile,
  change: { kind: "lines", added: 2, removed: 3 },
  gaps: [],
  rest: null,
  pairs: [
    { row: 3, removed: 3, added: 1 },
    { row: 8, removed: 1, added: 0 },
    { row: 10, removed: 0, added: 1 },
  ],
};

/** A choice of the side-by-side view. */
const sideBySide = () => {
  const choice = new DiffViewChoice();
  choice.toggle();
  return choice;
};

it("draws a change's removed rows beside its added ones side by side, and context on both sides", async () => {
  core.pullRequestRows.mockResolvedValue(ok(changed));
  await shown(ready(changed.length, { files: [changedFile] }), sideBySide());
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾a.py+2 −3");
  });

  expect(lines()).toEqual([
    "▾a.py+2 −3",
    "@@ -1,6 +1,5 @@",
    "1 a1 a",
    "2-b2+B",
    "3-c",
    "No newline at end of file",
    "4 d3 d",
    "5-e",
    "6 f4 f",
    "5+g",
  ]);
  /** What each side of row `row` shows: a removed line, an added one, or nothing. */
  const sides = (row: number) =>
    [...(document.querySelectorAll(".row")[row]?.children ?? [])].map(
      (side) =>
        ["removed", "added"].find((kind) => side.classList.contains(kind)) ?? (side.textContent || "nothing"),
    );
  expect([3, 4, 5, 7, 9].map(sides)).toEqual([
    ["removed", "added"],
    ["removed", "nothing"],
    ["No newline at end of file", "nothing"],
    ["removed", "nothing"],
    ["nothing", "added"],
  ]);
  const diff = screen.getByRole("region", { name: "Side-by-side diff" });
  const spacer = diff.querySelector(".spacer") as HTMLElement;
  expect(spacer.style.height).toBe(`${String(10 * 20)}px`);
  // As wide as the tab, which jsdom makes 0 wide, and as far again as a side's longest line runs past
  // its half.
  expect(spacer.style.minWidth).toBe("calc(0px + max(0px, 9ch - 0px + 56px))");
});

it("draws the lines a gap left out on both sides, side by side", async () => {
  setUpGapped();
  const paired = { ...gappedFile, pairs: [{ row: 3, removed: 1, added: 1 }] };
  await shown(ready(gapped.length, { files: [paired] }), sideBySide());
  await vi.waitFor(() => {
    expect(lines()).toContain("↓Whole file");
  });

  await fireEvent.click(screen.getByRole("button", { name: "Show 20 more lines" }));

  await vi.waitFor(() => {
    expect(lines()).toContain("109 def f109109 def f109");
  });
  expect(lines()).toContain("51-b51+B");
});

it("keeps the row at the top in view as the view switches", async () => {
  core.pullRequestRows.mockResolvedValue(ok(changed));
  const choice = new DiffViewChoice();
  await shown(ready(changed.length, { files: [changedFile] }), choice);
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾a.py+2 −3");
  });
  const diff = screen.getByRole("region", { name: "Diff" });
  // Five pixels into row 7, the context line "d", which shows at place 6 side by side.
  diff.scrollTop = 7 * 20 + 5;
  await fireEvent.scroll(diff);

  choice.toggle();
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(6 * 20 + 5);
  });
  await fireEvent.scroll(diff);
  choice.toggle();
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(7 * 20 + 5);
  });
  // At the top, the tab stays there.
  diff.scrollTop = 0;
  await fireEvent.scroll(diff);
  choice.toggle();
  await vi.waitFor(() => {
    expect(screen.getByRole("region", { name: "Side-by-side diff" })).toBe(diff);
  });
  expect(diff.scrollTop).toBe(0);
});

/** Thread `id` under row `row`, or at `row` when it isn't a number, with `comments` comments. */
const threadAt = (
  id: string,
  row: number | DiffThread["at"],
  resolution: "open" | "resolved",
  comments = 1,
): DiffThread => ({
  id,
  at: typeof row === "number" ? { kind: "row", row } : row,
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

/** The diff of `rows` with an open thread of two comments under row 2, which takes 7 places until it's
 * measured, and a resolved one under row 4, folded to one place: row 5 shows at place 13. */
const threadedDiff = () =>
  ready(rows.length, { threads: [threadAt("T1", 2, "open", 2), threadAt("T2", 4, "resolved")] });

/** Waits until what the last change set going has drawn. */
const settledTab = () => new Promise((resolve) => setTimeout(resolve, 0));

const threadBoxes = () => screen.getAllByRole("group", { name: "Review thread" });

it("draws each thread under its line, and a resolved one folded until a click opens it", async () => {
  await shown(threadedDiff());
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });

  expect(threadBoxes().map((box) => box.style.top)).toEqual([`${String(3 * 20)}px`, `${String(12 * 20)}px`]);
  const rowAt = (text: string) =>
    [...document.querySelectorAll<HTMLElement>(".row")].find((row) => row.textContent === text);
  expect(rowAt("2-old line")?.style.top).toBe(`${String(10 * 20)}px`);
  const folded = screen.getByRole("button", { name: "some-reviewer · 1 comment, Resolved" });
  expect(folded.getAttribute("aria-expanded")).toBe("false");

  await fireEvent.click(folded);

  expect(folded.getAttribute("aria-expanded")).toBe("true");
  expect(threadBoxes()[1]?.textContent).toContain("Why?");
});

it("measures each open thread, and keeps the row at the top in view as a thread above it shrinks", async () => {
  const observed: { element: Element; heard: () => void }[] = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      readonly #heard: () => void;
      constructor(heard: () => void) {
        this.#heard = heard;
      }
      observe(element: Element) {
        observed.push({ element, heard: this.#heard });
      }
      unobserve() {}
      disconnect() {}
    },
  );
  await shown(threadedDiff());
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });
  const diff = screen.getByRole("region", { name: "Diff" });
  // A thread below the top that opens leaves the tab where it is.
  diff.scrollTop = 20 + 5;
  await fireEvent.scroll(diff);
  await fireEvent.click(screen.getByRole("button", { name: "some-reviewer · 1 comment, Resolved" }));
  await settledTab();
  expect(diff.scrollTop).toBe(20 + 5);
  await fireEvent.click(screen.getByRole("button", { name: "some-reviewer · 1 comment, Resolved" }));
  // Five pixels into row 5, the second file's header.
  diff.scrollTop = 13 * 20 + 5;
  await fireEvent.scroll(diff);
  const [open] = threadBoxes();
  Object.defineProperty(open, "offsetHeight", { value: 95 });

  // The open thread now takes 5 places, and the folded one stays at one.
  for (const { heard } of observed) heard();

  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(11 * 20 + 5);
  });
  expect(threadBoxes()[1]?.style.top).toBe(`${String(10 * 20)}px`);
});

it("scrolls to the thread it moves to, with a few rows of its code above it", async () => {
  const files = await shown(threadedDiff());
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });
  const diff = screen.getByRole("region", { name: "Diff" });

  // The resolved thread opens, and takes 4 places from place 12.
  files.reveal("T2");
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe((12 - 3) * 20);
  });
  expect(files.jump).toBeUndefined();
  // A thread the layout doesn't show leaves the tab where it is.
  files.jump = { id: "gone" };
  await settledTab();
  expect(diff.scrollTop).toBe((12 - 3) * 20);
  expect(files.jump).toBeUndefined();
});

it("scrolls to the change it moves to, with a few rows of code above it", async () => {
  core.pullRequestRows.mockResolvedValue(ok(changed));
  const files = await shown(ready(changed.length, { files: [changedFile] }));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾a.py+2 −3");
  });

  files.nextChange(1);
  files.nextChange(1);

  await vi.waitFor(() => {
    expect(screen.getByRole("region", { name: "Diff" }).scrollTop).toBe((8 - 3) * 20);
  });
});

/** The core's answer to a search, which found `matches`, each a row and where in it the text starts and
 * ends. */
const found = (matches: [number, number, number][], more = false) =>
  ok({ matches: matches.map(([row, start, end]) => ({ row, start, end })), more });

/** The text of each match the rows mark, and whether it is the one the tab is on. */
const marks = () =>
  [...document.querySelectorAll(".row .match, .row .current")].map((mark) => [
    mark.textContent,
    mark.classList.contains("current"),
  ]);

/** The Files tab of `changed`, in the view `choice` holds, with its search open, once its rows came. */
const searching = async (choice = new DiffViewChoice()) => {
  core.pullRequestRows.mockResolvedValue(ok(changed));
  const files = await shown(ready(changed.length, { files: [changedFile] }), choice);
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾a.py+2 −3");
  });
  files.openSearch();
  return { files, box: await screen.findByRole("textbox", { name: "Find" }) };
};

it("searches every row from its box, marks the matches, and moves between them with Enter and its buttons", async () => {
  core.pullRequestFind.mockResolvedValue(
    found([
      [3, 0, 1],
      [6, 0, 1],
    ]),
  );
  const { box } = await searching();
  const status = () => screen.getByRole("search").querySelector(".count")?.textContent;

  await fireEvent.input(box, { target: { value: "b" } });
  await vi.waitFor(() => {
    expect(status()).toBe("1 of 2");
  });
  expect(marks()).toEqual([
    ["b", true],
    ["B", false],
  ]);
  await fireEvent.keyDown(box, { key: "Enter" });
  expect([status(), marks()]).toEqual([
    "2 of 2",
    [
      ["b", false],
      ["B", true],
    ],
  ]);
  await fireEvent.keyDown(box, { key: "Enter", shiftKey: true });
  expect(status()).toBe("1 of 2");
  await fireEvent.click(screen.getByRole("button", { name: "Next match" }));
  expect(status()).toBe("2 of 2");
  await fireEvent.click(screen.getByRole("button", { name: "Previous match" }));
  expect(status()).toBe("1 of 2");
  await fireEvent.keyDown(box, { key: "a" });
  expect(status()).toBe("1 of 2");
});

it("says when it finds nothing, with nowhere to move, and when the core stopped counting", async () => {
  core.pullRequestFind.mockResolvedValueOnce(found([]));
  const { box } = await searching();
  const status = () => screen.getByRole("search").querySelector(".count")?.textContent;

  await fireEvent.input(box, { target: { value: "zz" } });
  await vi.waitFor(() => {
    expect(status()).toBe("No results");
  });
  const moves = ["Previous match", "Next match"].map((name) => screen.getByRole("button", { name }));
  expect(moves.map((button) => (button as HTMLButtonElement).disabled)).toEqual([true, true]);
  core.pullRequestFind.mockResolvedValueOnce(found([[3, 0, 1]], true));
  await fireEvent.input(box, { target: { value: "b" } });
  await vi.waitFor(() => {
    expect(status()).toBe("1 of 1+");
  });
});

it("closes the search on Escape or its button, handing the diff its focus back", async () => {
  const { files, box } = await searching();

  await fireEvent.keyDown(box, { key: "Escape" });
  expect(screen.queryByRole("search")).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("region", { name: "Diff" }));
  files.openSearch();
  await fireEvent.click(await screen.findByRole("button", { name: "Close" }));
  expect(screen.queryByRole("search")).toBeNull();
});

it("marks the matches side by side, a line of context's on both sides", async () => {
  core.pullRequestFind.mockResolvedValue(
    found([
      [2, 0, 1],
      [3, 0, 1],
      [6, 0, 1],
    ]),
  );
  const { box } = await searching(sideBySide());

  await fireEvent.input(box, { target: { value: "x" } });

  await vi.waitFor(() => {
    expect(marks()).toEqual([
      ["a", true],
      ["a", true],
      ["b", false],
      ["B", false],
    ]);
  });
});

it("says in the box why it couldn't search", async () => {
  core.pullRequestFind.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
  const { box } = await searching();

  await fireEvent.input(box, { target: { value: "b" } });

  await vi.waitFor(() => {
    expect(screen.getByRole("search").querySelector(".count")?.textContent).toBe("Can't search: gone");
  });
  expect(screen.queryByRole("alert")).toBeNull();
});

it("leaves a match it shows in place, and scrolls to one it doesn't", async () => {
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(100);
  core.pullRequestFind.mockResolvedValue(
    found([
      [3, 0, 1],
      [10, 0, 1],
    ]),
  );
  const { box } = await searching();
  const diff = screen.getByRole("region", { name: "Diff" });

  // Rows 0 to 4 show in 100 pixels, and the tab draws the rest near them.
  await fireEvent.input(box, { target: { value: "x" } });
  await vi.waitFor(() => {
    expect(marks()).toEqual([
      ["b", true],
      ["g", false],
    ]);
  });
  expect(diff.scrollTop).toBe(0);
  await fireEvent.keyDown(box, { key: "Enter" });
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe((10 - 3) * 20);
  });
});

/** The Files tab of PR 11 once the core answered `answer`, one file at a time. */
const shownOne = async (answer: PrFiles) => {
  core.pullRequestFiles.mockResolvedValue(ok(answer));
  const files = new PrFilesState("python/cpython", 11, untimed, new DiffViewChoice(), "one");
  await files.load();
  render(FilesTab, { files, now: NOW, onlink });
  return files;
};

const bar = () => screen.getByRole("navigation", { name: "File on show" });
const previousFile = () => screen.getByRole<HTMLButtonElement>("button", { name: "Previous file" });
const nextFile = () => screen.getByRole<HTMLButtonElement>("button", { name: "Next file" });

it("shows one file at a time under a bar that counts the files, and walks them with its arrows", async () => {
  await shownOne(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });

  expect(bar().textContent.replace(/\s+/g, " ").trim()).toBe("‹1 of 2› news.txt");
  expect(lines()).toHaveLength(5);
  expect(previousFile().disabled).toBe(true);
  expect(nextFile().title).toBe("Next file (Alt+PageDown)");
  expect(screen.getByRole("button", { name: "One file at a time" }).getAttribute("aria-pressed")).toBe(
    "true",
  );
  await fireEvent.click(nextFile());
  await vi.waitFor(() => {
    expect(lines()).toEqual(["▾logo.pngbinary", "Binary files differ", "ast.py+0 −0"]);
  });
  expect(bar().textContent.replace(/\s+/g, " ").trim()).toBe("‹2 of 2› logo.png");
  expect(nextFile().disabled).toBe(true);
  await fireEvent.click(previousFile());
  expect(bar().textContent).toContain("1 of 2");
});

it("switches between all the files and one at a time from the header, keeping the row at the top", async () => {
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });
  const diff = screen.getByRole("region", { name: "Diff" });
  const toggle = screen.getByRole("button", { name: "One file at a time" });
  expect(toggle.getAttribute("aria-pressed")).toBe("false");
  expect(screen.queryByRole("navigation", { name: "File on show" })).toBeNull();
  // Five pixels into row 6, of the second file, which shows at its page's place 1.
  diff.scrollTop = 6 * 20 + 5;
  await fireEvent.scroll(diff);

  await fireEvent.click(toggle);
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(20 + 5);
  });
  expect([files.mode, bar().textContent.includes("2 of 2")]).toEqual(["one", true]);
  await fireEvent.scroll(diff);
  await fireEvent.click(toggle);
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(6 * 20 + 5);
  });
});

it("scrolls to the header of the next file with all the files, and to the very top for the NEWS entries", async () => {
  const news: NewsEntry[] = [{ path: "logo.png", section: "Library", issue: null, blocks: [] }];
  const files = await shown(ready(rows.length, { news }));
  await vi.waitFor(() => {
    expect(lines()[0]).toBe("▾notes.txt → news.txt+1 −1");
  });
  const diff = screen.getByRole("region", { name: "Diff" });

  files.nextFile(1);
  files.nextFile(1);
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(5 * 20);
  });
  await fireEvent.scroll(diff);
  files.nextFile(-1);
  await vi.waitFor(() => {
    expect(diff.scrollTop).toBe(0);
  });
  await fireEvent.scroll(diff);
  files.nextFile(-1);
  await vi.waitFor(() => {
    expect(files.jump).toBeUndefined();
  });
  // A page the layout doesn't show leaves the tab where it is.
  diff.scrollTop = 40;
  files.jump = { page: { kind: "file", file: 9 } };
  await settledTab();
  expect([diff.scrollTop, files.jump]).toEqual([40, undefined]);
});

it("puts the NEWS entries on a page of their own, and a NEWS entry's Show file goes to its file", async () => {
  const news: NewsEntry[] = [{ path: "logo.png", section: "Library", issue: null, blocks: [] }];
  await shownOne(ready(rows.length, { news }));

  expect(bar().textContent.replace(/\s+/g, " ").trim()).toBe("‹› NEWS entries");
  screen.getByRole("article", { name: "NEWS entry in Library" });
  expect(lines()).toEqual([]);
  await fireEvent.click(screen.getByRole("button", { name: "Show file" }));

  await vi.waitFor(() => {
    expect(bar().textContent).toContain("2 of 2");
  });
  expect(screen.queryByRole("region", { name: "NEWS entries" })).toBeNull();
});

it("gives the threads on files no longer in the diff a last page of their own", async () => {
  const files = await shownOne(
    ready(rows.length, {
      threads: [threadAt("lost", { kind: "gone", path: "Lib/old.py", code: null }, "open")],
    }),
  );
  files.showFile(1);
  await fireEvent.click(nextFile());

  await vi.waitFor(() => {
    expect(lines()).toEqual(["On files no longer in the diff"]);
  });
  expect(bar().textContent.replace(/\s+/g, " ").trim()).toBe("‹› On files no longer in the diff");
  expect(threadBoxes()).toHaveLength(1);
});

/** Clicks `element` with the caret `offset` characters into its text, where Chromium leaves a collapsed
 * selection after a click; jsdom lays nothing out to find the caret from. */
const clickAt = async (element: Element, offset: number) => {
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  let left = offset;
  let node = walker.nextNode() as Text | null;
  while (node !== null && left > node.length) {
    left -= node.length;
    node = walker.nextNode() as Text | null;
  }
  getSelection()?.collapse(node ?? element, node === null ? 0 : left);
  await fireEvent.click(element);
};

/** The code of the row `row` of the tab, on its `half` side by side. */
const codeOf = (row: number, half = 0) =>
  document.querySelectorAll(".row")[row]?.querySelectorAll(".text")[half] as Element;

/** The button in the header of the diff's second file, which a click collapses below the rows these tests
 * click. */
const secondHeader = () => screen.getByRole("button", { name: "▾logo.png" });

/** The picked word on each line of code that has one, as its pieces join up around the caret, and the text
 * of the row it is on. */
const picked = () =>
  [...document.querySelectorAll(".text")]
    .filter((code) => code.querySelector(".picked") !== null)
    .map((code) => [
      [...code.querySelectorAll(".picked")].map((piece) => piece.textContent).join(""),
      code.closest(".row")?.textContent,
    ]);

it("puts the caret on the word a click lands on and highlights that word alone", async () => {
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });

  // Six characters in, past the marker and inside "line".
  await clickAt(codeOf(4), 6);

  expect(picked()).toEqual([["line", "2+new line"]]);
  expect(codeOf(4).querySelectorAll(".caret")).toHaveLength(1);
  expect(lines()[4]).toBe("2+new line");
  expect(files.picked).toMatchObject({ word: "line", side: "head", line: 2 });
  await clickAt(codeOf(3), 2);
  expect(picked()).toEqual([["old", "2-old line"]]);
  expect(files.picked).toMatchObject({ word: "old", side: "base", line: 2 });
});

it("drops the pick on a click on no word, keeps it through a drag or a button, and never picks a hunk", async () => {
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);

  const range = document.createRange();
  range.selectNodeContents(codeOf(3));
  getSelection()?.removeAllRanges();
  getSelection()?.addRange(range);
  await fireEvent.click(codeOf(3));
  expect(files.picked?.word).toBe("new");
  await clickAt(secondHeader(), 0);
  expect(files.picked?.word).toBe("new");

  // Before the row's marker.
  await clickAt(codeOf(4), 0);
  expect([files.picked, picked()]).toEqual([undefined, []]);
  await clickAt(codeOf(4), 2);
  await clickAt(codeOf(1), 4);
  expect(files.picked).toBeUndefined();
  await clickAt(codeOf(4), 2);
  await clickAt(document.querySelectorAll(".row")[4]?.querySelector(".number") as Element, 0);
  expect(files.picked).toBeUndefined();

  // Past the end of a line, or on a number the user can't select, the browser leaves the caret in code.
  const row = document.querySelectorAll(".row")[4] as Element;
  for (const target of [row, row.querySelector(".number") as Element]) {
    await clickAt(codeOf(4), 9);
    expect(files.picked?.word).toBe("line");
    await fireEvent.click(target);
    expect(files.picked).toBeUndefined();
  }
});

it("selects nothing on a double-click whose first click picked no word", async () => {
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 6);
  getSelection()?.removeAllRanges();

  await clickAt(secondHeader(), 0);
  await fireEvent.dblClick(codeOf(4), { detail: 2 });
  await tick();

  expect(files.picked?.caret).toBe(5);
  expect(getSelection()?.toString()).toBe("");
  // Nor on one whose word went before the double-click came, as with a newer diff.
  await clickAt(codeOf(4), 6);
  files.dropPick();
  await fireEvent.dblClick(codeOf(4), { detail: 2 });
  await tick();
  expect([files.picked, getSelection()?.toString()]).toEqual([undefined, ""]);
});

it("selects the word on a double-click, however far the browser's own word runs", async () => {
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });

  await clickAt(codeOf(4), 6);
  // The browser selects the whole line, as its word runs through the space.
  getSelection()?.selectAllChildren(codeOf(4));
  await fireEvent.click(codeOf(4), { detail: 2 });
  await fireEvent.dblClick(codeOf(4), { detail: 2 });

  await vi.waitFor(() => {
    expect(getSelection()?.toString()).toBe("line");
  });
  expect(files.picked).toMatchObject({ word: "line", caret: null });
  expect(picked()).toEqual([["line", "2+new line"]]);
  expect(codeOf(4).querySelectorAll(".caret")).toHaveLength(0);
});

it("picks a word side by side on the half it was clicked on, and on the lines a gap left out", async () => {
  setUpGapped();
  const files = await shown(ready(gapped.length, { files: [gappedFile] }), sideBySide());
  await vi.waitFor(() => {
    expect(lines()[2]).toBe("50 a50 a");
  });

  // A line of context shows on both halves, and only the one clicked on takes the pick.
  await clickAt(codeOf(2, 1), 1);
  expect(files.picked).toMatchObject({ word: "a", side: "head", line: 50, column: "right" });
  expect(codeOf(2, 0).querySelector(".picked")).toBeNull();
  expect(codeOf(2, 1).querySelector(".picked")?.textContent).toBe("a");
  await clickAt(codeOf(3, 0), 1);
  expect(files.picked).toMatchObject({ word: "b", side: "base", line: 51 });

  await fireEvent.click(screen.getAllByRole("button", { name: "Show 20 more lines" })[0] as HTMLElement);
  await vi.waitFor(() => {
    expect(lines()).toContain("109 def f109109 def f109");
  });
  const expanded = lines().indexOf("109 def f109109 def f109");
  await clickAt(codeOf(expanded, 0), 6);
  expect(files.picked).toMatchObject({ word: "f109", side: "head", line: 109, column: "left" });
  expect(picked()).toEqual([["f109", "109 def f109109 def f109"]]);
});

const peekBox = () => screen.queryByRole("group", { name: "Definition peek" });
/** The words of the peek's header, and of the line under it. */
const peekText = () =>
  [".header", ".body"].map((part) => peekBox()?.querySelector(part)?.textContent.replace(/\s+/g, " ").trim());
const rowAt = (text: string) =>
  [...document.querySelectorAll<HTMLElement>(".row")].find((row) => row.textContent === text);
/** The peek's code lines that show: each line's number and text, marked with `*` for the hit's. */
const peekCode = () =>
  [...(peekBox()?.querySelectorAll<HTMLElement>(".line") ?? [])].map(
    (line) => `${line.classList.contains("hit") ? "*" : ""}${line.textContent}`,
  );

/** A file of `count` lines, line N of which reads `line N`, as the core reads a hit's file. */
const fileOf = (count: number) => {
  core.definitionCode.mockImplementation((code) => {
    const lines = [];
    for (let line = code.start; line < Math.min(code.end, count + 1); line++)
      lines.push(`line ${String(line)}`);
    return Promise.resolve(ok({ count, lines }));
  });
};

it("opens the Definition peek under the picked line, names the hit, shows its code and pushes the rows after it down", async () => {
  const hit = (path: string, line: number, text: string) => ({
    place: { kind: "repository" as const, path, side: "head" as const, commit: "abc" },
    line,
    column: 4,
    text,
  });
  core.definitions.mockResolvedValue(
    ok({
      state: "found",
      hits: [hit("Lib/news.py", 12, "def new(text):"), hit("Lib/old.py", 3, "new = make()")],
    }),
  );
  fileOf(30);
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);

  const looking = files.peekDefinition();
  await tick();
  expect(peekText()).toEqual(["new at abc · the PR's head ✕", "Searching for the definition of new…"]);
  await looking;
  await vi.waitFor(() => {
    expect(peekCode()).toContain("*12line 12");
  });

  expect(peekText()[0]).toBe("new Lib/news.py:12 at abc · the PR's head 1 of 2 ✕");
  expect(peekCode().slice(0, 3)).toEqual(["1line 1", "2line 2", "3line 3"]);
  expect(core.definitionCode).toHaveBeenCalledWith({
    repository: "python/cpython",
    file: { kind: "repository", path: "Lib/news.py", commit: "abc" },
    start: 1,
    end: 201,
  });
  expect(peekBox()?.style.top).toBe(`${String(5 * 20)}px`);
  expect(peekBox()?.style.height).toBe(`${String(18 * 20)}px`);
  expect(rowAt("▾logo.pngbinary")?.style.top).toBe(`${String(23 * 20)}px`);
});

it("scrolls the peek's code inside it, and the diff's rows after it stay where they are", async () => {
  const place = { kind: "repository" as const, path: "Objects/big.c", side: "base" as const, commit: "abc" };
  core.definitions.mockResolvedValue(
    ok({ state: "found", hits: [{ place, line: 900, column: 0, text: "" }] }),
  );
  fileOf(2000);
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);
  await files.peekDefinition();
  const code = await vi.waitFor(() => screen.getByRole("region", { name: "Definition's code" }));

  // The hit's line sits three rows from the top.
  expect(code.scrollTop).toBe((900 - 1 - 3) * 20);
  await vi.waitFor(() => {
    expect(peekCode()).toContain("*900line 900");
  });
  code.scrollTop = 1500 * 20;
  await fireEvent.scroll(code);
  await vi.waitFor(() => {
    expect(peekCode()).toContain("1500line 1500");
  });

  // Only the chunks near what shows are read, not the whole file.
  expect(core.definitionCode.mock.calls.map(([asked]) => asked.start)).toEqual([801, 1401]);
  expect(rowAt("▾logo.pngbinary")?.style.top).toBe(`${String(23 * 20)}px`);
});

it("names a hit outside the Repository by its whole path on disk, in place of a commit", async () => {
  const place = { kind: "outside" as const, path: "C:/ty/typeshed/stdlib/builtins.pyi" };
  core.definitions.mockResolvedValue(
    ok({ state: "found", hits: [{ place, line: 7, column: 4, text: "def print(" }] }),
  );
  fileOf(10);
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);

  await files.peekDefinition();
  await vi.waitFor(() => {
    expect(peekCode()).toContain("*7line 7");
  });

  expect(peekText()[0]).toBe("new C:/ty/typeshed/stdlib/builtins.pyi:7 1 of 1 ✕");
  expect(core.definitionCode).toHaveBeenCalledWith({
    repository: "python/cpython",
    file: { kind: "outside", path: "C:/ty/typeshed/stdlib/builtins.pyi" },
    start: 1,
    end: 201,
  });
});

it("says how to set ty up, and when no server knows the file's language", async () => {
  core.definitions.mockResolvedValueOnce(ok({ state: "not_set_up" }));
  core.definitions.mockResolvedValueOnce(ok({ state: "no_server" }));
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);

  await files.peekDefinition();
  await tick();
  const notSetUp = peekText();
  await files.peekDefinition();
  await tick();

  expect(notSetUp).toEqual(["new at abc · the PR's head ✕", "Set up ty to go to definitions in Python"]);
  expect(peekText()).toEqual([
    "new at abc · the PR's head ✕",
    "Tsugiki can't go to definitions in this file's language yet",
  ]);
});

it("says when a lookup found no definition, at the base for a removed line, or why it failed", async () => {
  core.definitions.mockResolvedValueOnce(ok({ state: "found", hits: [] }));
  core.definitions.mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "ty couldn't answer: the server ended before it answered" },
  });
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });

  await clickAt(codeOf(3), 2);
  await files.peekDefinition();
  await tick();
  expect(peekText()).toEqual(["old at base · the PR's base ✕", "No definition of old"]);
  await clickAt(codeOf(4), 2);
  await files.peekDefinition();
  await tick();
  expect(peekText()).toEqual([
    "new at abc · the PR's head ✕",
    "ty couldn't answer: the server ended before it answered",
  ]);
});

it("closes the Definition peek with its ✕, and gives the diff the focus", async () => {
  core.definitions.mockResolvedValue(ok({ state: "found", hits: [] }));
  const files = await shown(ready(rows.length));
  await vi.waitFor(() => {
    expect(lines()[4]).toBe("2+new line");
  });
  await clickAt(codeOf(4), 2);
  await files.peekDefinition();
  await tick();
  const close = peekBox()?.querySelector("button") as HTMLButtonElement;
  close.focus();

  await fireEvent.click(close);

  expect(peekBox()).toBeNull();
  expect(document.activeElement).toBe(screen.getByRole("region", { name: "Diff" }));
  expect(rowAt("▾logo.pngbinary")?.style.top).toBe(`${String(5 * 20)}px`);
});
