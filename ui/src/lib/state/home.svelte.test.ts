import { beforeEach, expect, it, vi } from "vitest";
import { commands, type RepositorySettings_Serialize } from "$lib/data/bindings";
import { HomeState } from "./home.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { home: vi.fn(), openRepository: vi.fn() } }));

beforeEach(() => {
  vi.resetAllMocks();
});

it("holds what the core says the window opens on", async () => {
  vi.mocked(commands.home).mockResolvedValue({ screen: "onboarding", token: "gh" });
  const state = new HomeState();
  expect(state.current).toBeUndefined();
  await state.load();
  expect(state.current).toEqual({ screen: "onboarding", token: "gh" });
});

it("says so when it can't ask", async () => {
  vi.mocked(commands.home).mockRejectedValue(new Error("the core has gone"));
  const state = new HomeState();
  await state.load();
  expect(state.current).toBeUndefined();
  expect(state.error).toBe("Error: the core has gone");
});

const peps: RepositorySettings_Serialize = { name: "python/peps", conventions: "github", filters: [] };
const cpython: RepositorySettings_Serialize = { name: "python/cpython", conventions: "cpython", filters: [] };

it("switches to the Repository the user picked, as the core opens it", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "ok",
    data: { screen: "repository", repository: peps },
  });
  const state = new HomeState();
  state.current = { screen: "repository", repository: cpython };
  await state.open("python/peps");
  expect(commands.openRepository).toHaveBeenCalledWith("python/peps");
  expect(state.current).toEqual({ screen: "repository", repository: peps });
  expect(state.switchProblem).toBeUndefined();
});

it("stays on the open Repository and says why when the core refuses a switch, until one works", async () => {
  vi.mocked(commands.openRepository).mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
  });
  const state = new HomeState();
  state.current = { screen: "repository", repository: cpython };
  await state.open("python/peps");
  expect(state.current).toEqual({ screen: "repository", repository: cpython });
  expect(state.switchProblem).toBe(
    "Tsugiki couldn't switch to python/peps: machine.toml, line 1: expected a value",
  );

  vi.mocked(commands.openRepository).mockResolvedValueOnce({
    status: "ok",
    data: { screen: "repository", repository: peps },
  });
  await state.open("python/peps");
  expect(state.switchProblem).toBeUndefined();
});

it("opens the Repository the user added, and forgets why a switch failed before", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
  });
  const state = new HomeState();
  await state.open("python/peps");

  state.added({ screen: "repository", repository: peps });

  expect(state.current).toEqual({ screen: "repository", repository: peps });
  expect(state.switchProblem).toBeUndefined();
});
