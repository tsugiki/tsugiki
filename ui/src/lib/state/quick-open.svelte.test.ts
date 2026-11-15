import { beforeEach, expect, it, vi } from "vitest";
import { commands, type TitledItem_Serialize } from "$lib/data/bindings";
import { FILES_LISTED, filesFound, QuickOpenState } from "./quick-open.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { quickOpen: vi.fn() } }));

const core = vi.mocked(commands);

const item = (number: number): TitledItem_Serialize => ({
  kind: "issue",
  number,
  title: `Issue ${String(number)}`,
  state: "OPEN",
});

beforeEach(() => {
  vi.resetAllMocks();
});

it("asks the core about the open Repository, and holds its answer with the text it answers", async () => {
  core.quickOpen.mockResolvedValue({ status: "ok", data: [item(7)] });
  const search = new QuickOpenState("python/cpython");

  await search.find("7");

  expect(core.quickOpen).toHaveBeenCalledWith("python/cpython", "7");
  expect(search.answer).toEqual({ typed: "7", found: [item(7)] });
});

/** The answers to the searches the core is asked, which each test gives in its own time. */
function heldAnswers() {
  const answers: ((found: TitledItem_Serialize[]) => void)[] = [];
  core.quickOpen.mockImplementation(
    () =>
      new Promise((resolve) => {
        answers.push((data) => {
          resolve({ status: "ok", data });
        });
      }),
  );
  return answers;
}

it("asks one search at a time, then only the latest text typed while it ran", async () => {
  const answers = heldAnswers();
  const search = new QuickOpenState("python/cpython");
  const first = search.find("1");
  void search.find("12");
  void search.find("123");
  const askedWhileRunning = core.quickOpen.mock.calls.length;

  answers[0]?.([item(1)]);
  await vi.waitFor(() => {
    expect(answers).toHaveLength(2);
  });
  const afterFirst = search.answer;
  answers[1]?.([item(123)]);
  await first;

  expect(askedWhileRunning).toBe(1);
  expect(afterFirst).toEqual({ typed: "1", found: [item(1)] });
  expect(core.quickOpen.mock.calls).toEqual([
    ["python/cpython", "1"],
    ["python/cpython", "123"],
  ]);
  expect(search.answer).toEqual({ typed: "123", found: [item(123)] });

  // Once the searches have answered, the next text asks at once.
  core.quickOpen.mockResolvedValue({ status: "ok", data: [] });
  await search.find("1234");
  expect(search.answer).toEqual({ typed: "1234", found: [] });
});

it("asks nothing more when the text typed meanwhile is back to the one under way", async () => {
  const answers = heldAnswers();
  const search = new QuickOpenState("python/cpython");
  const first = search.find("1");
  void search.find("12");
  void search.find("1");

  answers[0]?.([item(1)]);
  await first;

  expect(core.quickOpen.mock.calls).toEqual([["python/cpython", "1"]]);
  expect(search.answer).toEqual({ typed: "1", found: [item(1)] });
});

it("drops the text waiting for the search under way once it stops", async () => {
  const answers = heldAnswers();
  const search = new QuickOpenState("python/cpython");
  const first = search.find("1");
  void search.find("12");

  search.stop();
  answers[0]?.([item(1)]);
  await first;

  expect(core.quickOpen.mock.calls).toEqual([["python/cpython", "1"]]);
});

it("says why the core couldn't search", async () => {
  core.quickOpen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
  const search = new QuickOpenState("python/cpython");

  await search.find("7");

  expect(search.answer).toEqual({ typed: "7", problem: "Tsugiki can't search the Issues and PRs: gone" });
});

/** The files of a PR on show, which hold `paths` and note each read of the diff. */
const changedFiles = (paths: readonly string[]) => ({ load: vi.fn(), paths: () => paths, open: vi.fn() });

it("reads the diff of the PR on show as it searches, and holds the PR's files", async () => {
  core.quickOpen.mockResolvedValue({ status: "ok", data: [] });
  const files = changedFiles(["Lib/ast.py"]);
  const search = new QuickOpenState("python/cpython", files);
  const unread = files.load.mock.calls.length;

  await search.find("ast");

  expect([unread, files.load.mock.calls.length]).toEqual([0, 1]);
  expect(search.files).toBe(files);
});

it("finds the paths that hold every word typed, in any case, in the order the Files tab shows them", () => {
  const files = changedFiles([
    "Lib/ast.py",
    "Lib/test/test_ast.py",
    "Python/ast_opt.c",
    "Doc/library/ast.rst",
  ]);

  expect(filesFound(files, "AST")).toEqual([
    "Lib/ast.py",
    "Lib/test/test_ast.py",
    "Python/ast_opt.c",
    "Doc/library/ast.rst",
  ]);
  expect(filesFound(files, " lib  .py ")).toEqual(["Lib/ast.py", "Lib/test/test_ast.py"]);
  expect(filesFound(files, "#7")).toEqual([]);
});

it("finds every file for nothing typed, 20 at most", () => {
  const paths = Array.from({ length: FILES_LISTED + 5 }, (_, index) => `Lib/f${String(index)}.py`);

  expect(filesFound(changedFiles(paths), "")).toEqual(paths.slice(0, FILES_LISTED));
});
