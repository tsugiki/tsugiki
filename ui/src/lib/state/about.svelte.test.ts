import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { AboutState } from "./about.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { about: vi.fn() } }));

describe("AboutState", () => {
  it("holds what the core says about itself once loaded", async () => {
    vi.mocked(commands.about).mockResolvedValue({ name: "Tsugiki", version: "1.2.3" });
    const state = new AboutState();
    expect(state.about).toBeUndefined();
    await state.load();
    expect(state.about).toEqual({ name: "Tsugiki", version: "1.2.3" });
    expect(state.error).toBeUndefined();
  });

  it("holds the error when the core can't be reached", async () => {
    vi.mocked(commands.about).mockRejectedValue(new Error("no IPC outside Tauri"));
    const state = new AboutState();
    await state.load();
    expect(state.about).toBeUndefined();
    expect(state.error).toBe("Error: no IPC outside Tauri");
  });

  it("forgets an earlier failure when asked again", async () => {
    const state = new AboutState();
    vi.mocked(commands.about).mockRejectedValueOnce(new Error("not yet"));
    await state.load();
    vi.mocked(commands.about).mockResolvedValueOnce({ name: "Tsugiki", version: "1.2.3" });
    await state.load();
    expect(state.about).toEqual({ name: "Tsugiki", version: "1.2.3" });
    expect(state.error).toBeUndefined();
  });
});
