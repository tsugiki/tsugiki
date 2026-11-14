import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { type LoadedSettings, SettingsState } from "./settings.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { settings: vi.fn() } }));

describe("SettingsState", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("waits for the core before showing the status bar, so a hidden one doesn't appear for a moment", () => {
    const state = new SettingsState();
    expect(state.statusBarVisible).toBe(false);
    expect(state.problems).toEqual([]);
    expect(state.diffFiles).toBe("all");
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
        diff: { files: "one" },
        repositories: [],
      },
      problems: ["settings.toml, line 3: expected a value"],
    });
    const state = new SettingsState();
    await state.load();
    expect(state.statusBarVisible).toBe(false);
    expect(state.diffFiles).toBe("one");
    expect(state.problems).toEqual(["settings.toml, line 3: expected a value"]);
  });

  it("keeps what it last read when it can't ask again", async () => {
    vi.mocked(commands.settings).mockRejectedValue(new Error("the core has gone"));
    const state = new SettingsState();
    state.take({
      settings: {
        status_bar: { visible: false },
        releases: { check: true },
        appearance: {
          theme: "system",
          light_theme: "VS Code Light Modern",
          dark_theme: "VS Code Dark Modern",
        },
        diff: { files: "one" },
        repositories: [],
      },
      problems: [],
    });
    await state.load();
    expect(state.statusBarVisible).toBe(false);
    expect(state.diffFiles).toBe("one");
    expect(state.problems).toEqual(["Tsugiki can't read its settings: Error: the core has gone"]);
  });

  it("says so when it can't ask, and keeps the status bar", async () => {
    vi.mocked(commands.settings).mockRejectedValue(new Error("the core has gone"));
    const state = new SettingsState();
    await state.load();
    expect(state.statusBarVisible).toBe(true);
    expect(state.problems).toEqual(["Tsugiki can't read its settings: Error: the core has gone"]);
  });

  /** What the core read, with the status bar `visible` or not. */
  const read = (visible: boolean): LoadedSettings => ({
    settings: {
      status_bar: { visible },
      releases: { check: true },
      appearance: { theme: "system", light_theme: "VS Code Light Modern", dark_theme: "VS Code Dark Modern" },
      diff: { files: "all" },
      repositories: [],
    },
    problems: [],
  });

  it("leaves settings taken after a load asked, such as the Settings dialog's", async () => {
    const answer = Promise.withResolvers<LoadedSettings>();
    vi.mocked(commands.settings).mockReturnValue(answer.promise);
    const state = new SettingsState();
    const loading = state.load();
    state.take(read(false));
    answer.resolve(read(true));
    await loading;
    expect(state.statusBarVisible).toBe(false);
  });

  it("leaves the Settings dialog's changes while they show, and loads again once it closes", async () => {
    vi.mocked(commands.settings).mockResolvedValue(read(true));
    const state = new SettingsState();
    state.take(read(true));
    state.preview(true);
    // The dialog's change, which isn't a take.
    state.settings = read(false).settings;
    await state.load();
    expect(state.statusBarVisible).toBe(false);

    state.preview(false);
    await state.load();
    expect(state.statusBarVisible).toBe(true);
  });
});
