import { beforeEach, expect, it, vi } from "vitest";
import { commands, type RepositoryCheck_Serialize } from "$lib/data/bindings";
import { AddRepositoryState } from "./add-repository.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { checkRepository: vi.fn(), addRepository: vi.fn() } }));

const check: RepositoryCheck_Serialize = {
  repository: { name: "Some-Maintainer/gcmon", conventions: "github", filters: [] },
  open_issues: 10,
  open_prs: 3,
  size: "small",
  window_days: 30,
  first_sync_items: 48,
  first_sync_seconds: 5,
  first_sync_points: 12,
  access: "maintained",
};

const notFound = { kind: "not_found", message: "GitHub has no Repository named a/b." } as const;

beforeEach(() => {
  vi.resetAllMocks();
});

it("asks about what the user typed and holds the answer", async () => {
  vi.mocked(commands.checkRepository).mockResolvedValue({ status: "ok", data: check });
  const state = new AddRepositoryState();
  state.input = "some-maintainer/gcmon";

  const checking = state.check();
  expect(state.step).toEqual({ step: "checking" });
  await checking;

  expect(commands.checkRepository).toHaveBeenCalledWith("some-maintainer/gcmon");
  expect(state.step).toEqual({ step: "checked", check });
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
  vi.mocked(commands.checkRepository).mockResolvedValue({ status: "ok", data: check });
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
    data: RepositoryCheck_Serialize;
  }>();
  vi.mocked(commands.checkRepository).mockReturnValue(promise);
  const state = new AddRepositoryState();
  state.input = "a/b";
  const checking = state.check();

  state.input = "a/bc";
  answer({ status: "ok", data: check });
  await checking;

  expect(state.step).toEqual({ step: "typing" });
});

it("adds by the name GitHub gave and returns what the window opens on", async () => {
  const home = { screen: "repository", repository: check.repository } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "ok", data: home });
  const state = new AddRepositoryState();

  const adding = state.add(check);
  expect(state.step).toEqual({ step: "adding", check });

  expect(await adding).toEqual(home);
  expect(commands.addRepository).toHaveBeenCalledWith("Some-Maintainer/gcmon", "maintained");
});

it("says why an add failed", async () => {
  const refused = { kind: "internal", message: "settings.toml, line 3: expected a value" } as const;
  vi.mocked(commands.addRepository).mockResolvedValue({ status: "error", error: refused });
  const state = new AddRepositoryState();

  expect(await state.add(check)).toBeUndefined();

  expect(state.step).toEqual({ step: "failed", problem: refused.message });
});
