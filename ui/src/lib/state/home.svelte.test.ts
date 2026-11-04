import { beforeEach, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { HomeState } from "./home.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { home: vi.fn() } }));

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
