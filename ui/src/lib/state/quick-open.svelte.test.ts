import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { type QuickOpenItem, QuickOpenState } from "./quick-open.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { quickOpen: vi.fn() } }));

const core = vi.mocked(commands);

const item = (number: number): QuickOpenItem => ({
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

it("asks one search at a time, then only the latest text typed while it ran", async () => {
  const answers: ((found: QuickOpenItem[]) => void)[] = [];
  core.quickOpen.mockImplementation(
    () =>
      new Promise((resolve) => {
        answers.push((data) => {
          resolve({ status: "ok", data });
        });
      }),
  );
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

it("says why the core couldn't search", async () => {
  core.quickOpen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
  const search = new QuickOpenState("python/cpython");

  await search.find("7");

  expect(search.answer).toEqual({ typed: "7", problem: "Tsugiki can't search the Issues and PRs: gone" });
});
