import { beforeEach, expect, it, vi } from "vitest";
import { commands, type NameCheck_Serialize } from "$lib/data/bindings";
import { openOn, repositoryCheck } from "$lib/testing/fixtures";
import { AddRepositoryState } from "./add-repository.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { checkRepository: vi.fn(), addRepository: vi.fn(), pickFolder: vi.fn(), checkClone: vi.fn() },
}));

// GitHub names the Repository as its owner spells it, whatever case the user typed.
const check = repositoryCheck("Some-Maintainer/gcmon");

const found: NameCheck_Serialize = { kind: "new", check };

const notFound = { kind: "not_found", message: "GitHub has no Repository named a/b." } as const;

beforeEach(() => {
  vi.resetAllMocks();
});

it("asks about what the user typed and holds the answer", async () => {
  vi.mocked(commands.checkRepository).mockResolvedValue({ status: "ok", data: found });
  const state = new AddRepositoryState();
  state.input = "some-maintainer/gcmon";

  const checking = state.check();
  expect(state.step).toEqual({ step: "checking" });
  await checking;

  expect(commands.checkRepository).toHaveBeenCalledWith("some-maintainer/gcmon");
  expect(state.step).toEqual({ step: "checked", check });
});

it("holds the name Tsugiki has a Repository by when the core says it has it", async () => {
  vi.mocked(commands.checkRepository).mockResolvedValue({
    status: "ok",
    data: { kind: "held", name: "python/cpython" },
  });
  const state = new AddRepositoryState();
  state.input = "Python/CPython";

  await state.check();

  expect(state.step).toEqual({ step: "held", name: "python/cpython" });
});

it("says what the core says when it refuses", async () => {
  vi.mocked(commands.checkRepository).mockResolvedValue({ status: "error", error: notFound });
  const state = new AddRepositoryState();
  await state.check();
  expect(state.step).toEqual({ step: "failed", problem: notFound.message });
});

it("says so when it can't ask", async () => {
  vi.mocked(commands.checkRepository).mockRejectedValue(new Error("the core has gone"));
  const state = new AddRepositoryState();
  await state.check();
  expect(state.step).toEqual({ step: "failed", problem: "Error: the core has gone" });
});

it("forgets a check when the text changes", async () => {
  vi.mocked(commands.checkRepository).mockResolvedValue({ status: "ok", data: found });
  const state = new AddRepositoryState();
  state.input = "a/b";
  await state.check();

  state.input = "a/bc";

  expect(state.input).toBe("a/bc");
  expect(state.step).toEqual({ step: "typing" });
});

it("ignores an answer about text the user has since changed", async () => {
  const { promise, resolve: answer } = Promise.withResolvers<{
    status: "ok";
    data: NameCheck_Serialize;
  }>();
  vi.mocked(commands.checkRepository).mockReturnValue(promise);
  const state = new AddRepositoryState();
  state.input = "a/b";
  const checking = state.check();

  state.input = "a/bc";
  answer({ status: "ok", data: found });
  await checking;

  expect(state.step).toEqual({ step: "typing" });
});

it("adds by the name GitHub gave and returns what the window opens on", async () => {
  const home = openOn(check.repository);
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();

  const adding = state.add(check);
  expect(state.step).toEqual({ step: "adding", check });

  expect(await adding).toEqual(home);
  expect(commands.addRepository).toHaveBeenCalledWith({
    name: "Some-Maintainer/gcmon",
    source: { kind: "download" },
    read_only: true,
  });
});

it("adds a Repository that can write once the user turns Read-only off, and keeps the choice as they type", async () => {
  const home = { screen: "repository", repository: check.repository, others: [] as string[] } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();

  state.readOnly = false;
  state.input = "other/repo";
  await state.add(check);

  expect(commands.addRepository).toHaveBeenCalledWith({
    name: "Some-Maintainer/gcmon",
    source: { kind: "download" },
    read_only: false,
  });
});

const lender = String.raw`D:\gcmon`;

/** The picker gives `folder`, and the core says whether it lends. */
function picking(folder: string | null, refusal: { kind: "no_remote" } | null = null) {
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: folder });
  vi.mocked(commands.checkClone).mockResolvedValue({ status: "ok", data: refusal });
}

it("adds with the clone the user picked, and forgets it when the text changes", async () => {
  const home = openOn(check.repository);
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();
  picking(lender);
  await state.mirror.chooseClone("Some-Maintainer/gcmon");

  await state.add(check);
  state.input = "other/repo";

  expect(commands.addRepository).toHaveBeenCalledWith({
    name: "Some-Maintainer/gcmon",
    source: { kind: "borrow", folder: lender },
    read_only: true,
  });
  expect(state.mirror.source).toEqual({ kind: "download" });
});

it("forgets a clone picked after the text changed", async () => {
  const state = new AddRepositoryState();
  picking(lender);
  const choosing = state.mirror.chooseClone("Some-Maintainer/gcmon");
  state.input = "other/repo";
  await choosing;
  expect(state.mirror.source).toEqual({ kind: "download" });
});

it("adds nothing while no clone lends, though the dialog's button stays off then", async () => {
  const state = new AddRepositoryState();
  picking(lender, { kind: "no_remote" });
  await state.mirror.chooseClone("Some-Maintainer/gcmon");

  expect(await state.add(check)).toBeUndefined();

  expect(commands.addRepository).not.toHaveBeenCalled();
  expect(state.step).toEqual({ step: "typing" });
});

it("says why an add failed", async () => {
  const refused = { kind: "internal", message: "settings.toml, line 3: expected a value" } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "error", error: refused });
  const state = new AddRepositoryState();

  expect(await state.add(check)).toBeUndefined();

  expect(state.step).toEqual({ step: "failed", problem: refused.message });
});
