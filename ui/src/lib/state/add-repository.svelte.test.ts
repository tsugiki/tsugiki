import { beforeEach, expect, it, vi } from "vitest";
import { commands, type NameCheck_Serialize, type RepositoryCheck_Serialize } from "$lib/data/bindings";
import { AddRepositoryState } from "./add-repository.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { checkRepository: vi.fn(), addRepository: vi.fn(), pickFolder: vi.fn(), checkClone: vi.fn() },
}));

const check: RepositoryCheck_Serialize = {
  repository: { name: "Some-Maintainer/gcmon", conventions: "github", filters: [] },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
  mirror_mb: 891,
  download_seconds: 209,
  borrow_seconds: 36,
  access: "maintained",
};

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
  const home = { screen: "repository", repository: check.repository, others: [] as string[] } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();

  const adding = state.add(check);
  expect(state.step).toEqual({ step: "adding", check });

  expect(await adding).toEqual(home);
  expect(commands.addRepository).toHaveBeenCalledWith({
    name: "Some-Maintainer/gcmon",
    access: "maintained",
    source: { kind: "download" },
  });
});

const lender = String.raw`D:\gcmon`;

/** The picker gives `folder`, and the core says whether it lends. */
function picking(folder: string | null, refusal: { kind: "no_remote" } | null = null) {
  vi.mocked(commands.pickFolder).mockResolvedValue({ status: "ok", data: folder });
  vi.mocked(commands.checkClone).mockResolvedValue({ status: "ok", data: refusal });
}

it("downloads the mirror unless the user picks a clone that lends", async () => {
  const state = new AddRepositoryState();
  expect(state.source).toEqual({ kind: "download" });

  picking(lender);
  await state.chooseClone("Some-Maintainer/gcmon");
  expect(state.source).toEqual({ kind: "borrow", folder: lender });

  state.chooseDownload();
  expect(state.source).toEqual({ kind: "download" });
});

it("has nothing to add with while the picker is open or the clone can't lend", async () => {
  const state = new AddRepositoryState();
  picking(lender, { kind: "no_remote" });

  const choosing = state.chooseClone("Some-Maintainer/gcmon");
  expect(state.mirror).toEqual({ kind: "borrow", picked: undefined });
  expect(state.source).toBeUndefined();
  await choosing;

  expect(state.mirror).toEqual({
    kind: "borrow",
    picked: { kind: "refused", folder: lender, refusal: { kind: "no_remote" } },
  });
  expect(state.source).toBeUndefined();
});

it("keeps what was chosen when the user cancels the picker", async () => {
  const state = new AddRepositoryState();
  picking(lender);
  await state.chooseClone("Some-Maintainer/gcmon");
  picking(null);
  await state.chooseClone("Some-Maintainer/gcmon");
  expect(state.source).toEqual({ kind: "borrow", folder: lender });
});

it("adds with the clone the user picked, and forgets it when the text changes", async () => {
  const home = { screen: "repository", repository: check.repository, others: [] as string[] } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();
  picking(lender);
  await state.chooseClone("Some-Maintainer/gcmon");

  await state.add(check);
  state.input = "other/repo";

  expect(commands.addRepository).toHaveBeenCalledWith({
    name: "Some-Maintainer/gcmon",
    access: "maintained",
    source: { kind: "borrow", folder: lender },
  });
  expect(state.source).toEqual({ kind: "download" });
});

it("adds nothing while no clone lends, though the dialog's button stays off then", async () => {
  const state = new AddRepositoryState();
  picking(lender, { kind: "no_remote" });
  await state.chooseClone("Some-Maintainer/gcmon");

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
