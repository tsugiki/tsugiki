import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { SettingsState } from "./settings.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { settings: vi.fn() } }));

describe("SettingsState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("waits for the core before showing the status bar, so a hidden one doesn't appear for a moment", () => {
    const state = new SettingsState();
    expect(state.statusBarVisible).toBe(false);
    expect(state.problems).toEqual([]);
  });

  it("holds what the core read", async () => {
    vi.mocked(commands.settings).mockResolvedValue({
      settings: {
        status_bar: { visible: false },
        releases: { check: true },
        appearance: {
          theme: "system",
          light_theme: "VS Code Light Modern",
          dark_theme: "VS Code Dark Modern",
        },
        repositories: [],
      },
      problems: ["settings.toml, line 3: expected a value"],
    });
    const state = new SettingsState();
    await state.load();
    expect(state.statusBarVisible).toBe(false);
    expect(state.problems).toEqual(["settings.toml, line 3: expected a value"]);
  });

  it("says so when it can't ask, and keeps the status bar", async () => {
    vi.mocked(commands.settings).mockRejectedValue(new Error("the core has gone"));
    const state = new SettingsState();
    await state.load();
    expect(state.statusBarVisible).toBe(true);
    expect(state.problems).toEqual(["Tsugiki can't read its settings: Error: the core has gone"]);
  });
});
