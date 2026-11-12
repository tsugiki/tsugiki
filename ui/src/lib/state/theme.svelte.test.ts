import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { applyTheme, type DarkQuery, ThemeState, type Themes, type ThemeView } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { themes: vi.fn(), saveTheme: vi.fn() } }));

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

const THEMES: Themes = {
  mode: "system",
  light: { chosen: "VS Code Light Modern", all: [LIGHT_MODERN, GRUVBOX_LIGHT] },
  dark: { chosen: "VS Code Dark Modern", all: [DARK_MODERN, GRUVBOX_DARK] },
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

  it("keeps the side Light or Dark mode names, whichever side Windows is on", async () => {
    for (const [mode, name] of [
      ["light", "VS Code Light Modern"],
      ["dark", "VS Code Dark Modern"],
    ] as const) {
      vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: { ...THEMES, mode } });
      const { query, turn } = sides(mode === "light");
      const state = new ThemeState(query);
      await state.load();
      expect(state.side, mode).toBe(mode);
      expect(state.current?.name, mode).toBe(name);
      turn(mode !== "light");
      expect(state.current?.name, mode).toBe(name);
    }
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
    expect(state.chosen("light")).toBeUndefined();
    expect(state.error).toBe("A built-in theme can't be read");
    await state.choose("light", "Gruvbox Light Hard");
    expect(commands.saveTheme).not.toHaveBeenCalled();
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
    expect(state.themes?.mode).toBe("system");
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
