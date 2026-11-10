import { beforeEach, expect, it, vi } from "vitest";
import { chooseMirrorSource, mirrorCost, pickClone } from "./mirror-source";
import { MirrorQuestionState } from "./mirror-question.svelte";

vi.mock("./mirror-source", () => ({ pickClone: vi.fn(), chooseMirrorSource: vi.fn(), mirrorCost: vi.fn() }));

const lender = String.raw`D:\cpython`;
const cost = { mirror_mb: 891, download_seconds: 209, borrow_seconds: 36 };

beforeEach(() => {
  vi.resetAllMocks();
});

it("asks GitHub what the mirror takes", async () => {
  vi.mocked(mirrorCost).mockResolvedValue(cost);
  const question = new MirrorQuestionState("python/cpython");
  expect(question.cost).toBeNull();

  await question.load();

  expect(question.cost).toEqual(cost);
  expect(mirrorCost).toHaveBeenCalledWith("python/cpython");
});

it("starts the clone with Download, and stays answered", async () => {
  const question = new MirrorQuestionState("python/cpython");
  expect(question.canStart).toBe(true);

  const starting = question.start();
  expect(question.canStart).toBe(false);

  expect(await starting).toBe(true);
  expect(chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "download" });
  expect(question.canStart).toBe(false);
  expect(await question.start()).toBe(false);
  expect(chooseMirrorSource).toHaveBeenCalledTimes(1);
});

it("starts the clone with a clone that lends", async () => {
  vi.mocked(pickClone).mockResolvedValue({ kind: "lends", folder: lender });
  const question = new MirrorQuestionState("python/cpython");
  await question.mirror.chooseClone("python/cpython");

  expect(await question.start()).toBe(true);

  expect(chooseMirrorSource).toHaveBeenCalledWith("python/cpython", { kind: "borrow", folder: lender });
});

it("starts nothing while the clone picked can't lend", async () => {
  vi.mocked(pickClone).mockResolvedValue({ kind: "refused", folder: lender, refusal: { kind: "no_remote" } });
  const question = new MirrorQuestionState("python/cpython");
  await question.mirror.chooseClone("python/cpython");

  expect(question.canStart).toBe(false);
  expect(await question.start()).toBe(false);
  expect(chooseMirrorSource).not.toHaveBeenCalled();
});

it("says why the answer didn't take, and lets the user try again", async () => {
  vi.mocked(chooseMirrorSource).mockResolvedValueOnce("machine.toml, line 1: expected a value");
  const question = new MirrorQuestionState("python/cpython");

  expect(await question.start()).toBe(false);
  expect(question.problem).toBe("machine.toml, line 1: expected a value");
  expect(question.canStart).toBe(true);

  expect(await question.start()).toBe(true);
  expect(question.problem).toBeUndefined();
});
