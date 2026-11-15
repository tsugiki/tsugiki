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

it("shows each answer as it comes, but never an older text's over a newer one's", async () => {
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
  const second = search.find("12");
  const third = search.find("123");

  answers[1]?.([item(12)]);
  await second;
  const afterSecond = search.answer;
  answers[0]?.([item(1)]);
  await first;
  const afterFirst = search.answer;
  answers[2]?.([item(123)]);
  await third;

  expect(afterSecond).toEqual({ typed: "12", found: [item(12)] });
  expect(afterFirst).toEqual(afterSecond);
  expect(search.answer).toEqual({ typed: "123", found: [item(123)] });
});

it("says why the core couldn't search", async () => {
  core.quickOpen.mockResolvedValue({ status: "error", error: { kind: "internal", message: "gone" } });
  const search = new QuickOpenState("python/cpython");

  await search.find("7");

  expect(search.answer).toEqual({ typed: "7", problem: "Tsugiki can't search the Issues and PRs: gone" });
});
