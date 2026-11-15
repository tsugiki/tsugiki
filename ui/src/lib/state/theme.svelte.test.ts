import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { applyTheme, type DarkQuery, ThemeState, type Themes, type ThemeView } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { themes: vi.fn(), saveTheme: vi.fn(), turnWindow: vi.fn() },
}));

// The window turns without saying a side, unless a test says otherwise.
beforeEach(() => {
  vi.mocked(commands.turnWindow).mockReset().mockResolvedValue(null);
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

  it("shows a pick for the side a mode from before keeps at once", async () => {
    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    const state = await loaded({ ...THEMES, theme: { kind: "side", side: "dark" } }, false);
    expect(state.side).toBe("dark");
    expect(state.current?.name).toBe("VS Code Dark Modern");

    await state.choose("dark", "Gruvbox Dark Hard");

    expect(state.current?.name).toBe("Gruvbox Dark Hard");
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
    expect(state.chosen("light")).toBeUndefined();
    expect(state.error).toBe("A built-in theme can't be read");
    await state.choose("light", "Gruvbox Light Hard");
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

  it("saves a side's choice, which the window then shows if that side is in use", async () => {
    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    const state = await loaded(THEMES, true);

    await state.choose("dark", "Gruvbox Dark Hard");
    await state.choose("light", "Gruvbox Light Hard");

    expect(vi.mocked(commands.saveTheme).mock.calls).toEqual([
      ["dark", "Gruvbox Dark Hard"],
      ["light", "Gruvbox Light Hard"],
    ]);
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(state.chosen("light")).toBe("Gruvbox Light Hard");
    expect(state.themes?.theme).toEqual({ kind: "system" });
    expect(state.problem).toBeUndefined();
  });

  it("puts the old choice back and says why when it can't be saved, until a save works", async () => {
    const state = await loaded(THEMES, true);
    vi.mocked(commands.saveTheme).mockResolvedValueOnce({
      status: "error",
      error: { kind: "internal", message: "settings.toml, line 2: expected `]`" },
    });
    vi.mocked(commands.saveTheme).mockRejectedValueOnce(new Error("the core has gone"));

    await state.choose("dark", "Gruvbox Dark Hard");
    expect(state.current?.name).toBe("VS Code Dark Modern");
    expect(state.problem).toBe("Tsugiki can't save the theme: settings.toml, line 2: expected `]`");
    await state.choose("dark", "Gruvbox Dark Hard");
    expect(state.problem).toBe("Tsugiki can't save the theme: Error: the core has gone");

    vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
    await state.choose("dark", "Gruvbox Dark Hard");
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(state.problem).toBeUndefined();
  });
});

describe("ThemeState's saves", () => {
  it("leave a newer choice in place when an older save fails after it", async () => {
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

    const first = state.choose("dark", "Gruvbox Dark Hard");
    await state.choose("dark", "Solarized Dark");
    failFirst({ status: "error", error: { kind: "internal", message: "the drive went away" } });
    await first;

    expect(state.current?.name).toBe("Solarized Dark");
    expect(state.problem).toBe("Tsugiki can't save the theme: the drive went away");
  });

  it("keep a choice made while a load was on its way, which read the settings before it", async () => {
    const state = await loaded(THEMES, true);
    let answer: (themes: { status: "ok"; data: Themes }) => void = () => {};
    vi.mocked(commands.themes).mockReturnValueOnce(
      new Promise((resolve) => {
        answer = resolve;
      }),
    );
    vi.mocked(commands.saveTheme).mockResolvedValueOnce({ status: "ok", data: null });

    const reload = state.load();
    await state.choose("dark", "Gruvbox Dark Hard");
    answer({ status: "ok", data: structuredClone(THEMES) });
    await reload;

    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    expect(state.chosen("light")).toBe("VS Code Light Modern");
  });

  it("put the old choice back on the themes a load brought while the save was on its way", async () => {
    const state = await loaded(THEMES, true);
    let fail: (answer: { status: "error"; error: { kind: "internal"; message: string } }) => void = () => {};
    vi.mocked(commands.saveTheme).mockReturnValueOnce(
      new Promise((resolve) => {
        fail = resolve;
      }),
    );

    const choice = state.choose("dark", "Gruvbox Dark Hard");
    await state.load();
    expect(state.current?.name).toBe("Gruvbox Dark Hard");
    fail({ status: "error", error: { kind: "internal", message: "the drive went away" } });
    await choice;

    expect(state.current?.name).toBe("VS Code Dark Modern");
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
