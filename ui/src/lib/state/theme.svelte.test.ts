import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { applyTheme, type DarkQuery, ThemeState, type Themes, type ThemeView } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { themes: vi.fn(), previewThemes: vi.fn(), saveTheme: vi.fn(), turnWindow: vi.fn() },
}));

/** What Settings › General shows of `[appearance]` when it shows a dark theme. */
const APPEARANCE = {
  theme: "Gruvbox Dark Hard",
  light_theme: "VS Code Light Modern",
  dark_theme: "VS Code Dark Modern",
};

// The window turns without saying a side, and saves work, unless a test says otherwise.
beforeEach(() => {
  vi.mocked(commands.turnWindow).mockReset().mockResolvedValue(null);
  vi.mocked(commands.saveTheme).mockReset().mockResolvedValue({ status: "ok", data: null });
  vi.mocked(commands.previewThemes).mockReset();
});

/** A theme of side `kind` named `name`, with background `bg`. */
const view = (name: string, kind: "light" | "dark", bg: string): ThemeView => ({
  name,
  kind,
  tokens: [
    { property: "--bg", value: bg },
    { property: "--hl-keyword", value: kind === "light" ? "#0000ff" : "#569cd6" },
  ],
});

const LIGHT_MODERN = view("VS Code Light Modern", "light", "#ffffff");
const GRUVBOX_LIGHT = view("Gruvbox Light Hard", "light", "#f9f5d7");
const DARK_MODERN = view("VS Code Dark Modern", "dark", "#1f1f1f");
const GRUVBOX_DARK = view("Gruvbox Dark Hard", "dark", "#1d2021");

const NAMED_DARK = { kind: "named", name: "Gruvbox Dark Hard", side: "dark" } as const;

const THEMES: Themes = {
  theme: { kind: "system" },
  light: { chosen: "VS Code Light Modern", all: [LIGHT_MODERN, GRUVBOX_LIGHT], failed: [] },
  dark: { chosen: "VS Code Dark Modern", all: [DARK_MODERN, GRUVBOX_DARK], failed: [] },
  problems: [],
};

/** Stands in for the window's `prefers-color-scheme` query, and turns Windows to the other side. */
const sides = (dark: boolean) => {
  let listener: ((e: { matches: boolean }) => void) | undefined;
  const query = {
    matches: dark,
    addEventListener: (_: string, l: (e: { matches: boolean }) => void) => {
      listener = l;
    },
  } as unknown as DarkQuery;
  return { query, turn: (matches: boolean) => listener?.({ matches }) };
};

/** A ThemeState that has the core's `themes`, with Windows on its dark side or not. */
const loaded = async (themes: Themes, dark: boolean) => {
  vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(themes) });
  const state = new ThemeState(sides(dark).query);
  await state.load();
  return state;
};

describe("ThemeState", () => {
  it("shows the chosen theme of the side Windows is on once the core sent them", async () => {
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: THEMES });
    const state = new ThemeState(sides(true).query);
    expect(state.current).toBeUndefined();
    await state.load();
    expect(state.side).toBe("dark");
    expect(state.current?.name).toBe("VS Code Dark Modern");
    expect(state.error).toBeUndefined();
  });

  it("shows the side's chosen theme, not its first", async () => {
    const themes = { ...THEMES, dark: { ...THEMES.dark, chosen: "Gruvbox Dark Hard" } };
    const state = await loaded(themes, true);
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(state.shown).toBe(state.current);
  });

  it("turns to the other theme when Windows changes sides", async () => {
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: THEMES });
    const { query, turn } = sides(false);
    const state = new ThemeState(query);
    await state.load();
    expect(state.current?.name).toBe("VS Code Light Modern");
    turn(true);
    expect(state.current?.name).toBe("VS Code Dark Modern");
    turn(false);
    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("shows the theme the settings name, whichever side Windows is on", async () => {
    for (const [side, name] of [
      ["light", "Gruvbox Light Hard"],
      ["dark", "Gruvbox Dark Hard"],
    ] as const) {
      const theme = { kind: "named", name, side } as const;
      vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: { ...THEMES, theme } });
      const { query, turn } = sides(side === "light");
      const state = new ThemeState(query);
      await state.load();
      expect(state.side, name).toBe(side);
      expect(state.current?.name, name).toBe(name);
      turn(side !== "light");
      expect(state.side, name).toBe(side);
      expect(state.current?.name, name).toBe(name);
    }
  });

  it("shows a pick over a mode from before at once", async () => {
    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    const state = await loaded({ ...THEMES, theme: { kind: "side", side: "dark" } }, false);
    expect(state.side).toBe("dark");
    expect(state.current?.name).toBe("VS Code Dark Modern");

    await state.pick(GRUVBOX_LIGHT);

    expect(state.current?.name).toBe("Gruvbox Light Hard");
    expect(state.side).toBe("light");
  });

  it("turns the window to the side of the theme in force after each answer", async () => {
    const state = await loaded(
      { ...THEMES, theme: { kind: "named", name: "Gruvbox Dark Hard", side: "dark" } },
      false,
    );
    vi.mocked(commands.themes).mockResolvedValue({
      status: "ok",
      data: { ...THEMES, theme: { kind: "side", side: "light" } },
    });
    await state.load();
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    await state.load();

    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([["dark"], ["light"], [null]]);
  });

  it("takes the side the window turned back to, which the media query hears of later", async () => {
    const state = await loaded(
      { ...THEMES, theme: { kind: "named", name: "Gruvbox Dark Hard", side: "dark" } },
      true,
    );
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    vi.mocked(commands.turnWindow).mockResolvedValueOnce("light");

    await state.load();

    expect(state.dark).toBe(false);
    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("keeps a side Windows changed to while the window turned back", async () => {
    vi.mocked(commands.themes).mockResolvedValue({
      status: "ok",
      data: { ...THEMES, theme: { kind: "named", name: "Gruvbox Light Hard", side: "light" } },
    });
    const { query, turn } = sides(false);
    const state = new ThemeState(query);
    await state.load();
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    let said: (side: "light") => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        said = resolve;
      }),
    );

    const reload = state.load();
    await vi.waitFor(() => expect(commands.turnWindow).toHaveBeenLastCalledWith(null));
    turn(true);
    said("light");
    await reload;

    expect(state.dark).toBe(true);
    expect(state.current?.name).toBe("VS Code Dark Modern");
  });

  it("takes the side the window said back on Windows' side though the webview catches up late", async () => {
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: { ...THEMES, theme: NAMED_DARK } });
    const { query, turn } = sides(false);
    const state = new ThemeState(query);
    await state.load();
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    let said: (side: "light") => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        said = resolve;
      }),
    );

    const reload = state.load();
    await vi.waitFor(() => expect(commands.turnWindow).toHaveBeenLastCalledWith(null));
    // The webview reports the dark side the window was kept on only now.
    turn(true);
    said("light");
    await reload;

    expect(state.dark).toBe(false);
    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("shows the themes only once the window said its side, so the page doesn't flash the side kept", async () => {
    const state = await loaded({ ...THEMES, theme: NAMED_DARK }, true);
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    let said: (side: "light") => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        said = resolve;
      }),
    );

    const reload = state.load();
    await vi.waitFor(() => expect(commands.turnWindow).toHaveBeenLastCalledWith(null));
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    said("light");
    await reload;

    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("asks the window to turn only when the side changes", async () => {
    const state = await loaded(THEMES, false);
    await state.load();
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: { ...THEMES, theme: NAMED_DARK } });
    await state.load();
    await state.load();

    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"]]);
  });

  it("shows the load asked last when two cross, on the page and in the window", async () => {
    const state = await loaded(THEMES, false);
    let first: (themes: { status: "ok"; data: Themes }) => void = () => {};
    vi.mocked(commands.themes)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          first = resolve;
        }),
      )
      .mockResolvedValueOnce({ status: "ok", data: { ...THEMES, theme: NAMED_DARK } });

    const older = state.load();
    await state.load();
    first({ status: "ok", data: { ...THEMES, theme: { kind: "side", side: "light" } } });
    await older;

    expect(state.themes?.theme).toEqual(NAMED_DARK);
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"]]);
  });

  it("sends each turn only once the one before it answered", async () => {
    const state = await loaded(THEMES, false);
    let said: (side: null) => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        said = resolve;
      }),
    );
    vi.mocked(commands.themes).mockResolvedValueOnce({
      status: "ok",
      data: { ...THEMES, theme: NAMED_DARK },
    });

    const older = state.load();
    await vi.waitFor(() => expect(commands.turnWindow).toHaveBeenLastCalledWith("dark"));
    vi.mocked(commands.themes).mockResolvedValueOnce({ status: "ok", data: structuredClone(THEMES) });
    const newer = state.load();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(commands.turnWindow).toHaveBeenCalledTimes(2);
    said(null);
    await older;
    await newer;

    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"], [null]]);
    expect(state.themes?.theme).toEqual({ kind: "system" });
  });

  it("turns the window back to the page's theme when a load fails after one it dropped had turned it", async () => {
    const state = await loaded(THEMES, false);
    let said: (side: null) => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        said = resolve;
      }),
    );
    vi.mocked(commands.themes).mockResolvedValueOnce({
      status: "ok",
      data: { ...THEMES, theme: NAMED_DARK },
    });

    const dropped = state.load();
    await vi.waitFor(() => expect(commands.turnWindow).toHaveBeenLastCalledWith("dark"));
    vi.mocked(commands.themes).mockResolvedValueOnce({
      status: "error",
      error: { kind: "internal", message: "The themes folder can't be read" },
    });
    const failed = state.load();
    said(null);
    await dropped;
    await failed;

    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(state.error).toBe("The themes folder can't be read");
    expect(commands.turnWindow).toHaveBeenLastCalledWith(null);
  });

  it("keeps the window on its side when the themes don't come, as the page keeps its theme", async () => {
    const named = { kind: "named", name: "Gruvbox Dark Hard", side: "dark" } as const;
    const state = await loaded({ ...THEMES, theme: named }, false);
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "The themes folder can't be read" },
    });

    await state.load();

    expect(state.error).toBe("The themes folder can't be read");
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([["dark"]]);
  });

  it("says why the window can't turn, goes on with the media query, and asks again on the next load", async () => {
    vi.mocked(commands.turnWindow).mockRejectedValueOnce(new Error("the window has gone"));
    const state = await loaded(THEMES, true);

    expect(state.dark).toBe(true);
    expect(state.current?.name).toBe("VS Code Dark Modern");
    expect(state.turnProblem).toBe(
      "Tsugiki can't turn its title bar to the theme's side: Error: the window has gone",
    );

    await state.load();

    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], [null]]);
    expect(state.turnProblem).toBeUndefined();
  });

  it("holds the core's refusal and shows no theme", async () => {
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "A built-in theme can't be read" },
    });
    const state = new ThemeState(sides(false).query);
    await state.load();
    expect(state.current).toBeUndefined();
    expect(state.shown).toBeUndefined();
    expect(state.themesOf("light")).toEqual([]);
    expect(state.failedOf("light")).toEqual([]);
    expect(state.problems).toEqual([]);
    expect(state.following).toBeUndefined();
    expect(state.error).toBe("A built-in theme can't be read");
    await state.pick(GRUVBOX_LIGHT);
    expect(commands.saveTheme).not.toHaveBeenCalled();
    expect(commands.turnWindow).not.toHaveBeenCalled();
  });

  it("reads the themes again on each load, with the files' problems and those that failed", async () => {
    const state = await loaded(THEMES, false);
    const failed = { name: "Night", file: "night.toml", reason: "it extends itself: Night, Night" };
    const problem = "themes\\mine.toml, line 6: fg isn't a colour, so Tsugiki skips it.";
    vi.mocked(commands.themes).mockResolvedValue({
      status: "ok",
      data: { ...THEMES, dark: { ...THEMES.dark, failed: [failed] }, problems: [problem] },
    });
    await state.load();
    expect(state.problems).toEqual([problem]);
    expect(state.failedOf("dark")).toEqual([failed]);
    expect(state.failedOf("light")).toEqual([]);
  });

  it("forgets a refusal once the core sends the themes", async () => {
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "No" },
    });
    const state = new ThemeState(sides(false).query);
    await state.load();
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    await state.load();
    expect(state.error).toBeUndefined();
    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("shows a preview over the theme in force until it's dropped", async () => {
    const state = await loaded(THEMES, true);
    state.preview = GRUVBOX_LIGHT;
    expect(state.shown).toEqual(GRUVBOX_LIGHT);
    expect(state.current?.name).toBe("VS Code Dark Modern");
    state.preview = undefined;
    expect(state.shown?.name).toBe("VS Code Dark Modern");
  });

  it("shows the themes the core worked out for the Settings dialog, under the picker's preview, on their side", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });

    await state.previewSettings(APPEARANCE);

    expect(commands.previewThemes).toHaveBeenCalledWith(APPEARANCE);
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
    expect(state.current?.name).toBe("VS Code Light Modern");
    expect(commands.turnWindow).toHaveBeenLastCalledWith("dark");
    state.preview = LIGHT_MODERN;
    expect(state.shown?.name).toBe("VS Code Light Modern");
    state.preview = undefined;

    expect(await state.endPreview()).toBeUndefined();
    expect(state.shown?.name).toBe("VS Code Light Modern");
    expect(commands.turnWindow).toHaveBeenLastCalledWith(null);
  });

  it("shows Follow system from the dialog on the side the window says the system is on", async () => {
    const state = await loaded({ ...THEMES, theme: NAMED_DARK }, true);
    vi.mocked(commands.previewThemes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    vi.mocked(commands.turnWindow).mockResolvedValue("light");

    await state.previewSettings({ ...APPEARANCE, theme: "system" });

    expect(state.shown?.name).toBe("VS Code Light Modern");
  });

  it("turns the window back to nothing when the dialog stops, if the window never had themes", async () => {
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });
    const state = new ThemeState(sides(false).query);
    await state.load();
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    await state.previewSettings(APPEARANCE);
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
    const turns = vi.mocked(commands.turnWindow).mock.calls.length;

    expect(await state.endPreview()).toBeUndefined();

    expect(state.shown).toBeUndefined();
    expect(vi.mocked(commands.turnWindow).mock.calls.length).toBe(turns);
  });

  it("asks for one change at a time, then only for the latest, and shows no answer for values changed since", async () => {
    const state = await loaded(THEMES, false);
    const first = Promise.withResolvers<{ status: "ok"; data: Themes }>();
    vi.mocked(commands.previewThemes).mockReturnValueOnce(first.promise);
    vi.mocked(commands.previewThemes).mockResolvedValueOnce({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    const lightHard = { ...APPEARANCE, theme: "Gruvbox Light Hard" };

    const asked = state.previewSettings(lightHard);
    void state.previewSettings({ ...APPEARANCE, theme: "system" });
    void state.previewSettings(APPEARANCE);
    await vi.waitFor(() => {
      expect(commands.previewThemes).toHaveBeenCalledOnce();
    });
    first.resolve({
      status: "ok",
      data: {
        ...structuredClone(THEMES),
        theme: { kind: "named", name: "Gruvbox Light Hard", side: "light" },
      },
    });
    await asked;

    expect(commands.previewThemes).toHaveBeenCalledTimes(2);
    expect(commands.previewThemes).toHaveBeenLastCalledWith(APPEARANCE);
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
    expect(commands.turnWindow).not.toHaveBeenCalledWith("light");
  });

  it("goes on asking after a call that throws", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockImplementationOnce(() => {
      throw new Error("no IPC");
    });

    await state.previewSettings(APPEARANCE);

    expect(state.previewProblem).toBe("Tsugiki can't show the theme: Error: no IPC");
    expect(await state.endPreview(APPEARANCE)).toBeUndefined();
  });

  it("turns the window with a load while the dialog has no themes to show", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });
    await state.previewSettings(APPEARANCE);
    vi.mocked(commands.themes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });

    await state.load();

    expect(commands.turnWindow).toHaveBeenLastCalledWith("dark");
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
  });

  it("keeps only the themes of the values Apply saved, and reads the themes for others", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    await state.previewSettings(APPEARANCE);
    const asked = vi.mocked(commands.themes).mock.calls.length;

    expect(await state.endPreview({ ...APPEARANCE, theme: "system" })).toBeUndefined();

    expect(vi.mocked(commands.themes).mock.calls.length).toBe(asked + 1);
    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(state.shown?.name).toBe("VS Code Light Modern");
  });

  it("answers Apply with its own read of the themes, not one an older load left", async () => {
    vi.mocked(commands.themes).mockResolvedValueOnce({
      status: "error",
      error: { kind: "internal", message: "an old refusal" },
    });
    const state = new ThemeState(sides(false).query);
    await state.load();
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });
    await state.previewSettings(APPEARANCE);
    const own = Promise.withResolvers<{ status: "ok"; data: Themes }>();
    vi.mocked(commands.themes).mockReturnValueOnce(own.promise);

    const ending = state.endPreview(APPEARANCE);
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(2);
    });
    expect(state.error).toBe("an old refusal");
    own.resolve({ status: "ok", data: structuredClone(THEMES) });

    expect(await ending).toBeUndefined();
    expect(state.error).toBeUndefined();
  });

  it("answers Apply with a later load that worked, not the refusal of its own that it crossed", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });
    await state.previewSettings(APPEARANCE);
    const own = Promise.withResolvers<{ status: "error"; error: { kind: "internal"; message: string } }>();
    const later = Promise.withResolvers<{ status: "ok"; data: Themes }>();
    vi.mocked(commands.themes).mockReturnValueOnce(own.promise).mockReturnValueOnce(later.promise);

    const ending = state.endPreview(APPEARANCE);
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(2);
    });
    void state.load();
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(3);
    });
    own.resolve({ status: "error", error: { kind: "internal", message: "the folder was busy" } });
    later.resolve({ status: "ok", data: { ...structuredClone(THEMES), theme: NAMED_DARK } });

    expect(await ending).toBeUndefined();
    expect(state.shown?.name).toBe(NAMED_DARK.name);
  });

  it("leaves the window's side to the Settings dialog while a load answers", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    await state.previewSettings(APPEARANCE);
    const turns = vi.mocked(commands.turnWindow).mock.calls.length;

    await state.load();

    expect(vi.mocked(commands.turnWindow).mock.calls.length).toBe(turns);
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
  });

  it("keeps the dialog's themes in force once Apply saved them, and reads the themes again", async () => {
    const state = await loaded(THEMES, false);
    const dialogs = { ...structuredClone(THEMES), theme: NAMED_DARK };
    vi.mocked(commands.previewThemes).mockResolvedValue({ status: "ok", data: dialogs });
    await state.previewSettings(APPEARANCE);
    const reading = Promise.withResolvers<{ status: "ok"; data: Themes }>();
    vi.mocked(commands.themes).mockReturnValue(reading.promise);

    expect(await state.endPreview(APPEARANCE)).toBeUndefined();

    expect(state.themes?.theme).toEqual(NAMED_DARK);
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(2);
    });
    reading.resolve({ status: "ok", data: { ...dialogs, problems: ["night.toml: bad"] } });
    await vi.waitFor(() => {
      expect(state.problems).toEqual(["night.toml: bad"]);
    });
  });

  it("says why it can't show the dialog's appearance, and why Apply's can't be shown either", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });
    await state.previewSettings(APPEARANCE);
    expect(state.previewProblem).toBe("Tsugiki can't show the theme: access denied");
    expect(state.shown?.name).toBe("VS Code Light Modern");
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });

    expect(await state.endPreview(APPEARANCE)).toBe(
      "Tsugiki saved the settings, but can't show the theme: access denied",
    );
    expect(state.previewProblem).toBeUndefined();
    expect(await state.endPreview(APPEARANCE)).toBeUndefined();
  });

  it("turns the window back to what it shows once an answer it turned for changed and the next was refused", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes)
      .mockResolvedValueOnce({ status: "ok", data: { ...structuredClone(THEMES), theme: NAMED_DARK } })
      .mockResolvedValueOnce({ status: "error", error: { kind: "internal", message: "access denied" } });
    const turning = Promise.withResolvers<null>();
    vi.mocked(commands.turnWindow).mockReturnValueOnce(turning.promise);

    const asked = state.previewSettings(APPEARANCE);
    await vi.waitFor(() => {
      expect(commands.turnWindow).toHaveBeenLastCalledWith("dark");
    });
    void state.previewSettings({ ...APPEARANCE, theme: "system" });
    turning.resolve(null);
    await asked;

    expect(state.previewProblem).toBe("Tsugiki can't show the theme: access denied");
    expect(state.shown?.name).toBe("VS Code Light Modern");
    expect(commands.turnWindow).toHaveBeenLastCalledWith(null);
  });

  it("goes on showing the dialog's themes until Apply's read of values it had none for answers", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    await state.previewSettings(APPEARANCE);
    const reading = Promise.withResolvers<{ status: "ok"; data: Themes }>();
    vi.mocked(commands.themes).mockReturnValueOnce(reading.promise);

    const ending = state.endPreview({ ...APPEARANCE, theme: "system" });
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(2);
    });
    expect(state.shown?.name).toBe("Gruvbox Dark Hard");
    expect(commands.turnWindow).toHaveBeenLastCalledWith("dark");
    reading.resolve({ status: "ok", data: structuredClone(THEMES) });

    expect(await ending).toBeUndefined();
    expect(state.shown?.name).toBe("VS Code Light Modern");
    expect(commands.turnWindow).toHaveBeenLastCalledWith(null);
  });

  it("shows the theme in force from the last read once Apply's can't be shown, and has nothing to put back", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.previewThemes).mockResolvedValue({
      status: "ok",
      data: { ...structuredClone(THEMES), theme: NAMED_DARK },
    });
    await state.previewSettings(APPEARANCE);
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "access denied" },
    });

    expect(await state.endPreview({ ...APPEARANCE, theme: "system" })).toBe(
      "Tsugiki saved the settings, but can't show the theme: access denied",
    );
    expect(state.shown?.name).toBe("VS Code Light Modern");
    expect(commands.turnWindow).toHaveBeenLastCalledWith(null);
    const turns = vi.mocked(commands.turnWindow).mock.calls.length;
    expect(await state.endPreview()).toBeUndefined();
    expect(vi.mocked(commands.turnWindow).mock.calls.length).toBe(turns);
  });

  it("follows the system with the chosen theme of the side Windows is on", async () => {
    const { query, turn } = sides(true);
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(THEMES) });
    const state = new ThemeState(query);
    await state.load();
    expect(state.following?.name).toBe("VS Code Dark Modern");
    turn(false);
    expect(state.following?.name).toBe("VS Code Light Modern");
  });

  it("saves a pick of the other side's theme as the theme in force, and turns the window to its side", async () => {
    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    const state = await loaded(THEMES, false);

    await state.pick(GRUVBOX_DARK);

    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith({ kind: "named", name: "Gruvbox Dark Hard" });
    expect(state.themes?.theme).toEqual(NAMED_DARK);
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(state.side).toBe("dark");
    expect(state.themes?.light.chosen).toBe("VS Code Light Modern");
    expect(state.themes?.dark.chosen).toBe("VS Code Dark Modern");
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"]]);
    expect(state.problem).toBeUndefined();
  });

  it("saves Follow system, turns the window back to Windows, and takes the side it says", async () => {
    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    const state = await loaded({ ...THEMES, theme: NAMED_DARK }, false);
    vi.mocked(commands.turnWindow).mockResolvedValueOnce("light");

    await state.pick("system");

    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith({ kind: "system" });
    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([["dark"], [null]]);
    expect(state.current?.name).toBe("VS Code Light Modern");
  });

  it("puts the old theme back and says why when it can't be saved, until a save works", async () => {
    const state = await loaded(THEMES, true);
    vi.mocked(commands.saveTheme).mockResolvedValueOnce({
      status: "error",
      error: { kind: "internal", message: "settings.toml, line 2: expected `]`" },
    });
    vi.mocked(commands.saveTheme).mockRejectedValueOnce(new Error("the core has gone"));

    await state.pick(GRUVBOX_LIGHT);
    expect(state.current?.name).toBe("VS Code Dark Modern");
    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["light"], [null]]);
    expect(state.problem).toBe("Tsugiki can't save the theme: settings.toml, line 2: expected `]`");
    await state.pick(GRUVBOX_LIGHT);
    expect(state.problem).toBe("Tsugiki can't save the theme: Error: the core has gone");

    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    await state.pick(GRUVBOX_LIGHT);
    expect(state.current?.name).toBe("Gruvbox Light Hard");
    expect(state.problem).toBeUndefined();
  });
});

/** The core's answer to a save, which a test holds with `Promise.withResolvers`. */
type SaveAnswer =
  { status: "ok"; data: null } | { status: "error"; error: { kind: "internal"; message: string } };
const SAVED: SaveAnswer = { status: "ok", data: null };
const REFUSED: SaveAnswer = { status: "error", error: { kind: "internal", message: "the drive went away" } };

describe("ThemeState's saves", () => {
  it("go on, and so do loads, after a call that throws", async () => {
    const state = await loaded(THEMES, false);
    vi.mocked(commands.saveTheme)
      .mockImplementationOnce(() => {
        throw new Error("no such command");
      })
      .mockResolvedValueOnce(SAVED);

    await state.pick(GRUVBOX_DARK);
    expect(state.problem).toBe("Tsugiki can't save the theme: Error: no such command");
    expect(state.themes?.theme).toEqual({ kind: "system" });
    await state.pick(GRUVBOX_LIGHT);
    await state.load();

    expect(commands.saveTheme).toHaveBeenCalledTimes(2);
    expect(state.problem).toBeUndefined();
    expect(commands.themes).toHaveBeenCalledTimes(2);
  });

  it("take the side the window turned back to as it says it, not once the save answers", async () => {
    const { query, turn } = sides(true);
    vi.mocked(commands.themes).mockResolvedValue({
      status: "ok",
      data: structuredClone({ ...THEMES, theme: NAMED_DARK }),
    });
    const state = new ThemeState(query);
    await state.load();
    const save = Promise.withResolvers<SaveAnswer>();
    vi.mocked(commands.saveTheme).mockReturnValueOnce(save.promise);
    vi.mocked(commands.turnWindow).mockResolvedValueOnce("light");

    const picking = state.pick("system");
    await vi.waitFor(() => {
      expect(state.dark).toBe(false);
    });
    // Windows changes sides while the save is on its way.
    turn(true);
    save.resolve(SAVED);
    await picking;

    expect(state.dark).toBe(true);
    expect(state.current?.name).toBe("VS Code Dark Modern");
  });

  it("go one after the other, in the order picked", async () => {
    const state = await loaded(THEMES, false);
    const first = Promise.withResolvers<SaveAnswer>();
    vi.mocked(commands.saveTheme).mockReturnValueOnce(first.promise).mockResolvedValueOnce(SAVED);

    const picks = Promise.all([state.pick(GRUVBOX_DARK), state.pick(GRUVBOX_LIGHT)]);
    await vi.waitFor(() => {
      expect(commands.saveTheme).toHaveBeenCalledOnce();
    });
    first.resolve(SAVED);
    await picks;

    expect(vi.mocked(commands.saveTheme).mock.calls).toEqual([
      [{ kind: "named", name: "Gruvbox Dark Hard" }],
      [{ kind: "named", name: "Gruvbox Light Hard" }],
    ]);
    expect(state.current?.name).toBe("Gruvbox Light Hard");
  });

  it("put back the theme an earlier pick saved when a later one fails", async () => {
    const state = await loaded(THEMES, false);
    const first = Promise.withResolvers<SaveAnswer>();
    vi.mocked(commands.saveTheme).mockReturnValueOnce(first.promise).mockResolvedValueOnce(REFUSED);

    const picks = Promise.all([state.pick(GRUVBOX_DARK), state.pick(GRUVBOX_LIGHT)]);
    await vi.waitFor(() => {
      expect(commands.saveTheme).toHaveBeenCalledOnce();
    });
    first.resolve(SAVED);
    await picks;

    expect(state.themes?.theme).toEqual(NAMED_DARK);
    expect(state.problem).toBe("Tsugiki can't save the theme: the drive went away");
  });

  it("leave a newer pick in place when an older save fails", async () => {
    const third = view("Solarized Dark", "dark", "#002b36");
    const state = await loaded(
      { ...THEMES, dark: { ...THEMES.dark, all: [...THEMES.dark.all, third] } },
      true,
    );
    let failFirst: (answer: {
      status: "error";
      error: { kind: "internal"; message: string };
    }) => void = () => {};
    vi.mocked(commands.saveTheme)
      .mockReturnValueOnce(
        new Promise((resolve) => {
          failFirst = resolve;
        }),
      )
      .mockResolvedValueOnce({ status: "ok", data: null });

    const picks = Promise.all([state.pick(GRUVBOX_DARK), state.pick(third)]);
    await vi.waitFor(() => {
      expect(commands.saveTheme).toHaveBeenCalledOnce();
    });
    failFirst({ status: "error", error: { kind: "internal", message: "the drive went away" } });
    await picks;

    expect(state.current?.name).toBe("Solarized Dark");
    // The newer save answered after the one that failed.
    expect(state.problem).toBeUndefined();
  });

  it("keep a pick made while a load was on its way, which read the settings before it", async () => {
    const state = await loaded(THEMES, false);
    let answer: (themes: { status: "ok"; data: Themes }) => void = () => {};
    vi.mocked(commands.themes).mockReturnValueOnce(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    vi.mocked(commands.saveTheme).mockResolvedValueOnce({ status: "ok", data: null });

    const reload = state.load();
    await state.pick(GRUVBOX_DARK);
    answer({ status: "ok", data: structuredClone(THEMES) });
    await reload;

    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"]]);
  });

  it("keep a pick made while a load's turn was on its way", async () => {
    const state = await loaded({ ...THEMES, theme: NAMED_DARK }, false);
    let turned: (side: null) => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        turned = resolve;
      }),
    );
    vi.mocked(commands.themes).mockResolvedValueOnce({ status: "ok", data: structuredClone(THEMES) });
    vi.mocked(commands.saveTheme).mockResolvedValueOnce({ status: "ok", data: null });

    const reload = state.load();
    await vi.waitFor(() => {
      expect(commands.turnWindow).toHaveBeenCalledTimes(2);
    });
    const picked = state.pick(GRUVBOX_DARK);
    turned(null);
    await Promise.all([reload, picked]);

    expect(state.themes?.theme).toEqual(NAMED_DARK);
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([["dark"], [null], ["dark"]]);
  });

  it("put the saved theme back when every save fails, not a pick whose save hadn't answered", async () => {
    const state = await loaded(THEMES, false);
    const fails: ((answer: { status: "error"; error: { kind: "internal"; message: string } }) => void)[] = [];
    vi.mocked(commands.saveTheme).mockImplementation(
      () =>
        new Promise((resolve) => {
          fails.push(resolve);
        }),
    );
    const failure = { status: "error", error: { kind: "internal", message: "expected `]`" } } as const;

    const first = state.pick(GRUVBOX_DARK);
    const second = state.pick(GRUVBOX_LIGHT);
    await vi.waitFor(() => {
      expect(fails).toHaveLength(1);
    });
    fails[0]?.(failure);
    await first;
    expect(state.current?.name).toBe("Gruvbox Light Hard");
    await vi.waitFor(() => {
      expect(fails).toHaveLength(2);
    });
    fails[1]?.(failure);
    await second;

    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(state.current?.name).toBe("VS Code Light Modern");
    expect(vi.mocked(commands.turnWindow).mock.calls.at(-1)).toEqual([null]);
  });

  it("let a load asked while a save is on its way read what the save wrote", async () => {
    const state = await loaded(THEMES, false);
    const save = Promise.withResolvers<SaveAnswer>();
    vi.mocked(commands.saveTheme).mockReturnValueOnce(save.promise).mockResolvedValueOnce(REFUSED);
    // The core answers with what the file holds.
    let written = false;
    vi.mocked(commands.themes).mockImplementation(async () => ({
      status: "ok",
      data: structuredClone(written ? { ...THEMES, theme: NAMED_DARK } : THEMES),
    }));

    const picking = state.pick(GRUVBOX_DARK);
    const reload = state.load();
    await vi.waitFor(() => {
      expect(commands.saveTheme).toHaveBeenCalledOnce();
    });
    expect(commands.themes).toHaveBeenCalledOnce();
    written = true;
    save.resolve(SAVED);
    await Promise.all([picking, reload]);
    expect(commands.themes).toHaveBeenCalledTimes(2);
    expect(state.themes?.theme).toEqual(NAMED_DARK);

    // The load read the file as the save left it, so a pick that fails puts back what the save wrote.
    await state.pick(GRUVBOX_LIGHT);
    expect(state.themes?.theme).toEqual(NAMED_DARK);
  });

  it("put the saved theme back before a load that waited for the save reads it", async () => {
    const state = await loaded(THEMES, false);
    let turned: (side: null) => void = () => {};
    vi.mocked(commands.turnWindow).mockReturnValueOnce(
      new Promise((resolve) => {
        turned = resolve;
      }),
    );
    const save = Promise.withResolvers<SaveAnswer>();
    vi.mocked(commands.saveTheme).mockReturnValueOnce(save.promise);

    const picking = state.pick(GRUVBOX_DARK);
    const reload = state.load();
    await vi.waitFor(() => {
      expect(commands.turnWindow).toHaveBeenCalledTimes(2);
    });
    save.resolve(REFUSED);
    await vi.waitFor(() => {
      expect(commands.themes).toHaveBeenCalledTimes(2);
    });
    turned(null);
    await Promise.all([picking, reload]);

    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(state.current?.name).toBe("VS Code Light Modern");
    expect(vi.mocked(commands.turnWindow).mock.calls).toEqual([[null], ["dark"], [null]]);
  });
});

describe("applyTheme", () => {
  it("sets each token on the root and the theme's kind as its color-scheme", () => {
    const root = document.createElement("div");
    applyTheme(root, DARK_MODERN);
    expect(root.style.getPropertyValue("--bg")).toBe("#1f1f1f");
    expect(root.style.getPropertyValue("--hl-keyword")).toBe("#569cd6");
    expect(root.style.colorScheme).toBe("dark");
    applyTheme(root, LIGHT_MODERN);
    expect(root.style.getPropertyValue("--bg")).toBe("#ffffff");
    expect(root.style.colorScheme).toBe("light");
  });
});
