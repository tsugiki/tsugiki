import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { AboutState } from "./about.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { about: vi.fn() } }));

describe("AboutState", () => {
  it("holds what the core says about itself once loaded", async () => {
    vi.mocked(commands.about).mockResolvedValue({
      name: "Tsugiki",
      version: "1.2.3",
      build: "release",
      source_ref: "v1.2.3",
    });
    const state = new AboutState();
    expect(state.about).toBeUndefined();
    await state.load();
    expect(state.about).toEqual({
      name: "Tsugiki",
      version: "1.2.3",
      build: "release",
      source_ref: "v1.2.3",
    });
    expect(state.error).toBeUndefined();
  });

  it("holds the error when the core can't be reached", async () => {
    vi.mocked(commands.about).mockRejectedValue(new Error("no IPC outside Tauri"));
    const state = new AboutState();
    await state.load();
    expect(state.about).toBeUndefined();
    expect(state.error).toBe("Error: no IPC outside Tauri");
  });

  it("names the build in the title once the core answers, and the app alone before", async () => {
    vi.mocked(commands.about).mockResolvedValue({
      name: "Tsugiki",
      version: "0.3.2 (6143447)",
      build: "release",
      source_ref: "v0.3.2",
    });
    const state = new AboutState();
    const loading = state.load();
    expect(state.title).toBe("Tsugiki");
    await loading;
    expect(state.title).toBe("Tsugiki 0.3.2 (6143447)");
    expect(state.problem).toBeUndefined();
  });

  it("names the app alone in the title, and says why, when the core doesn't answer", async () => {
    vi.mocked(commands.about).mockRejectedValue(new Error("no IPC"));
    const state = new AboutState();
    await state.load();
    expect(state.title).toBe("Tsugiki");
    expect(state.problem).toBe("The core didn't answer: Error: no IPC");
  });

  it("forgets an earlier failure when asked again", async () => {
    const state = new AboutState();
    vi.mocked(commands.about).mockRejectedValueOnce(new Error("not yet"));
    await state.load();
    vi.mocked(commands.about).mockResolvedValueOnce({
      name: "Tsugiki",
      version: "1.2.3",
      build: "release",
      source_ref: "v1.2.3",
    });
    await state.load();
    expect(state.about).toEqual({
      name: "Tsugiki",
      version: "1.2.3",
      build: "release",
      source_ref: "v1.2.3",
    });
    expect(state.error).toBeUndefined();
  });
});
