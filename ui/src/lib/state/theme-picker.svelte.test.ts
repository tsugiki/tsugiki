import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { type PickerRow, ThemePicker } from "./theme-picker.svelte";
import { type DarkQuery, ThemeState, type Themes, type ThemeView } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({
  commands: { themes: vi.fn(), saveTheme: vi.fn(), turnWindow: vi.fn(async () => null) },
}));

const view = (name: string, kind: "light" | "dark"): ThemeView => ({ name, kind, tokens: [] });

const THEMES: Themes = {
  theme: { kind: "system" },
  light: {
    chosen: "VS Code Light Modern",
    all: [view("VS Code Light Modern", "light"), view("Gruvbox Light Hard", "light")],
    failed: [],
  },
  dark: {
    chosen: "Gruvbox Dark Hard",
    all: [view("VS Code Dark Modern", "dark"), view("Gruvbox Dark Hard", "dark")],
    failed: [],
  },
  problems: [],
};

/** THEMES with a file of each side that loaded and one of each that failed. */
const WITH_FILES: Themes = {
  ...THEMES,
  light: {
    ...THEMES.light,
    all: [...THEMES.light.all, view("Solarized Light", "light")],
    failed: [{ name: "Dawn", file: "dawn.toml", reason: 'its kind is "day"' }],
  },
  dark: {
    ...THEMES.dark,
    all: [...THEMES.dark.all, view("Solarized Dark", "dark")],
    failed: [{ name: "Night", file: "night.toml", reason: "it extends Dusk, which no theme is named" }],
  },
};

/** The picker over themes the core sent, with Windows on its dark side or not. */
const opened = async (dark: boolean, themes: Themes = THEMES) => {
  vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(themes) });
  const query = { matches: dark, addEventListener: () => {} } as unknown as DarkQuery;
  const theme = new ThemeState(query);
  await theme.load();
  return { theme, picker: new ThemePicker(theme) };
};

/** Each row as it reads: a heading in brackets, the row in force with a star. */
const titles = (picker: ThemePicker) =>
  picker.rows.map((row) => {
    if (row.kind === "heading") return `[${row.title}]`;
    if ((row.kind === "system" || row.kind === "theme") && row.chosen) return `${row.title} *`;
    return row.title;
  });

/** The row titled `title` that picks: Follow system or a theme. */
const pickRow = (picker: ThemePicker, title: string) => {
  const row = picker.rows.find((one) => one.title === title);
  if (row?.kind !== "theme" && row?.kind !== "system") throw new Error(`no row to pick titled ${title}`);
  return row;
};

const rowTitled = (picker: ThemePicker, title: string): PickerRow => {
  const row = picker.rows.find((one) => one.title === title);
  if (!row) throw new Error(`no row ${title}`);
  return row;
};

beforeEach(() => {
  vi.mocked(commands.saveTheme).mockReset();
  vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
});

describe("ThemePicker", () => {
  it("lists Follow system, each side's themes under its heading, the files that failed, then Reload themes", async () => {
    const { picker } = await opened(true, WITH_FILES);
    expect(titles(picker)).toEqual([
      "Follow system *",
      "[Light]",
      "VS Code Light Modern",
      "Gruvbox Light Hard",
      "Solarized Light",
      "[Dark]",
      "VS Code Dark Modern",
      "Gruvbox Dark Hard",
      "Solarized Dark",
      "Dawn (dawn.toml)",
      "Night (night.toml)",
      "Reload themes",
    ]);
    expect(rowTitled(picker, "Night (night.toml)")).toEqual({
      kind: "failed",
      title: "Night (night.toml)",
      reason: "it extends Dusk, which no theme is named",
    });
  });

  it("marks the theme the settings name, whichever side Windows is on", async () => {
    const named = { kind: "named", name: "Gruvbox Light Hard", side: "light" } as const;
    const { picker } = await opened(true, { ...THEMES, theme: named });
    expect(titles(picker).filter((title) => title.endsWith("*"))).toEqual(["Gruvbox Light Hard *"]);
  });

  it("marks the chosen theme of the side a mode from before keeps", async () => {
    const { picker } = await opened(false, { ...THEMES, theme: { kind: "side", side: "dark" } });
    expect(titles(picker).filter((title) => title.endsWith("*"))).toEqual(["Gruvbox Dark Hard *"]);
  });

  it("shows the theme in force on a heading, a file that failed and Reload themes", async () => {
    const { theme, picker } = await opened(true, WITH_FILES);
    for (const title of ["Dark", "Night (night.toml)", "Reload themes"]) {
      picker.move(pickRow(picker, "Solarized Dark"));
      picker.move(rowTitled(picker, title));
      expect(theme.shown?.name, title).toBe("Gruvbox Dark Hard");
    }
  });

  it("shows the chosen theme of the side Windows is on for Follow system", async () => {
    const named = { kind: "named", name: "Gruvbox Light Hard", side: "light" } as const;
    const { theme, picker } = await opened(true, { ...THEMES, theme: named });
    // The page hears Windows on its dark side, though the theme in force is a light one.
    picker.move(pickRow(picker, "Follow system"));
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    expect(theme.current?.name).toBe("Gruvbox Light Hard");
  });

  it("reads the themes folder again on Reload themes, so an edited file shows at once", async () => {
    const { picker } = await opened(true);
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(WITH_FILES) });
    await picker.reload();
    expect(titles(picker)).toContain("Solarized Dark");
  });

  it("previews each theme it moves to on the whole window at once", async () => {
    const { theme, picker } = await opened(true);
    picker.move(pickRow(picker, "Gruvbox Light Hard"));
    expect(theme.shown?.name).toBe("Gruvbox Light Hard");
    expect(theme.current?.name).toBe("Gruvbox Dark Hard");
  });

  it("saves a dark theme picked while Windows is light as the theme in force, and leaves the sides' choices", async () => {
    const { theme, picker } = await opened(false);
    picker.move(pickRow(picker, "VS Code Dark Modern"));
    await picker.pick(pickRow(picker, "VS Code Dark Modern"));
    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith({
      kind: "named",
      name: "VS Code Dark Modern",
    });
    expect(theme.current?.name).toBe("VS Code Dark Modern");
    expect(theme.themes?.light.chosen).toBe("VS Code Light Modern");
    expect(theme.themes?.dark.chosen).toBe("Gruvbox Dark Hard");
    expect(theme.preview).toBeUndefined();
    expect(titles(picker).filter((title) => title.endsWith("*"))).toEqual(["VS Code Dark Modern *"]);
  });

  it("saves Follow system as the theme in force", async () => {
    const named = { kind: "named", name: "Gruvbox Light Hard", side: "light" } as const;
    const { theme, picker } = await opened(false, { ...THEMES, theme: named });
    await picker.pick(pickRow(picker, "Follow system"));
    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith({ kind: "system" });
    expect(theme.themes?.theme).toEqual({ kind: "system" });
    expect(theme.current?.name).toBe("VS Code Light Modern");
    expect(titles(picker)[0]).toBe("Follow system *");
  });

  it("puts the theme in force back when it closes without a pick", async () => {
    const { theme, picker } = await opened(true);
    picker.move(pickRow(picker, "VS Code Dark Modern"));
    picker.cancel();
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    expect(commands.saveTheme).not.toHaveBeenCalled();
  });
});
