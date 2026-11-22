import { beforeEach, expect, it, vi } from "vitest";
import { commands, type RepositorySettings_Serialize } from "$lib/data/bindings";
import { ok, openOn } from "$lib/testing/fixtures";
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
  vi.mocked(commands.openRepository).mockResolvedValue(ok(openOn(peps)));
  const state = new HomeState();
  state.current = openOn(cpython);
  await state.open("python/peps");
  expect(commands.openRepository).toHaveBeenCalledWith("python/peps");
  expect(state.current).toEqual(openOn(peps));
  expect(state.switchProblem).toBeUndefined();
});

it("stays on the open Repository and says why when the core refuses a switch, until one works", async () => {
  vi.mocked(commands.openRepository).mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
  });
  const state = new HomeState();
  state.current = openOn(cpython);
  await state.open("python/peps");
  expect(state.current).toEqual(openOn(cpython));
  expect(state.switchProblem).toBe(
    "Tsugiki couldn't switch to python/peps: machine.toml, line 1: expected a value",
  );

  vi.mocked(commands.openRepository).mockResolvedValueOnce(ok(openOn(peps)));
  await state.open("python/peps");
  expect(state.switchProblem).toBeUndefined();
});

it("drops a Repository the settings no longer hold from the others when the core refuses a switch to it", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "error",
    error: { kind: "not_found", message: "Tsugiki doesn't have python/peps." },
  });
  vi.mocked(commands.home).mockResolvedValue(openOn(cpython, ["a/b"]));
  const state = new HomeState();
  state.current = openOn(cpython, ["python/peps", "a/b"]);
  await state.open("python/peps");
  expect(state.current).toEqual(openOn(cpython, ["a/b"]));
});

it("keeps the open Repository's screen when, after a refused switch, the core opens on another", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "error",
    error: { kind: "not_found", message: "Tsugiki doesn't have python/peps." },
  });
  vi.mocked(commands.home).mockResolvedValue(openOn(peps));
  const state = new HomeState();
  state.current = openOn(cpython, ["python/peps"]);
  await state.open("python/peps");
  expect(state.current).toEqual(openOn(cpython, ["python/peps"]));
});

it("keeps the others as they were when the core can't say what it opens on after a refused switch", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "error",
    error: { kind: "not_found", message: "Tsugiki doesn't have python/peps." },
  });
  vi.mocked(commands.home).mockRejectedValue(new Error("the core has gone"));
  const state = new HomeState();
  state.current = openOn(cpython, ["python/peps"]);
  await state.open("python/peps");
  expect(state.current).toEqual(openOn(cpython, ["python/peps"]));
  expect(state.switchProblem).toBe(
    "Tsugiki couldn't switch to python/peps: Tsugiki doesn't have python/peps.",
  );
});

it("opens the Repository the user added, and forgets why a switch failed before", async () => {
  vi.mocked(commands.openRepository).mockResolvedValue({
    status: "error",
    error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
  });
  const state = new HomeState();
  await state.open("python/peps");

  state.added(openOn(peps));

  expect(state.current).toEqual(openOn(peps));
  expect(state.switchProblem).toBeUndefined();
});

it("lists the open Repository first on the Home page, then the others, each with when it was opened", () => {
  const state = new HomeState();
  expect(state.rows).toEqual([]);

  state.current = openOn(
    cpython,
    [{ name: "python/peps", opened_at: "2026-11-23T08:00:00Z" }, "a/never"],
    "2026-11-23T09:00:00Z",
  );

  expect(state.rows).toEqual([
    { name: "python/cpython", opened_at: "2026-11-23T09:00:00Z" },
    { name: "python/peps", opened_at: "2026-11-23T08:00:00Z" },
    { name: "a/never", opened_at: null },
  ]);
});

it("shows the Home page only over an open Repository, and leaves it for that one", () => {
  const state = new HomeState();
  state.current = { screen: "onboarding", token: "gh" };
  state.goHome();
  expect(state.page).toBe(false);

  state.current = openOn(cpython);
  state.goHome();
  expect(state.page).toBe(true);
  state.leave();
  expect(state.page).toBe(false);
});

it("leaves the Home page for the Repository a switch or an add opens, and stays on it when a switch fails", async () => {
  vi.mocked(commands.openRepository).mockResolvedValueOnce({
    status: "error",
    error: { kind: "internal", message: "machine.toml, line 1: expected a value" },
  });
  vi.mocked(commands.home).mockResolvedValue(openOn(cpython, ["python/peps"]));
  const state = new HomeState();
  state.current = openOn(cpython, ["python/peps"]);
  state.goHome();
  await state.open("python/peps");
  expect(state.page).toBe(true);

  vi.mocked(commands.openRepository).mockResolvedValueOnce(ok(openOn(peps, ["python/cpython"])));
  await state.open("python/peps");
  expect(state.page).toBe(false);

  state.goHome();
  state.added(openOn(cpython, ["python/peps"]));
  expect(state.page).toBe(false);
});
