import { beforeEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { ThemePicker } from "./theme-picker.svelte";
import { type DarkQuery, ThemeState, type Themes, type ThemeView } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { themes: vi.fn(), saveTheme: vi.fn() } }));

const view = (name: string, kind: "light" | "dark"): ThemeView => ({ name, kind, tokens: [] });

const THEMES: Themes = {
  mode: "system",
  light: {
    chosen: "VS Code Light Modern",
    all: [view("VS Code Light Modern", "light"), view("Gruvbox Light Hard", "light")],
  },
  dark: {
    chosen: "Gruvbox Dark Hard",
    all: [view("VS Code Dark Modern", "dark"), view("Gruvbox Dark Hard", "dark")],
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

/** Each row as it reads, the side's chosen theme with a star. */
const titles = (picker: ThemePicker) =>
  picker.rows.map((row) => (row.kind === "theme" && row.chosen ? `${row.title} *` : row.title));

/** The theme row titled `title`. */
const themeRow = (picker: ThemePicker, title: string) => {
  const row = picker.rows.find((one) => one.title === title);
  if (row?.kind !== "theme") throw new Error(`no theme row ${title}`);
  return row;
};

beforeEach(() => {
  vi.mocked(commands.saveTheme).mockReset();
  vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
});

describe("ThemePicker", () => {
  it("lists the themes of the side in use, marks the theme in force and opens on it", async () => {
    const { picker } = await opened(true);
    expect(picker.side).toBe("dark");
    expect(titles(picker)).toEqual(["VS Code Dark Modern", "Gruvbox Dark Hard *", "Light themes…"]);
    expect(picker.start).toBe(1);
  });

  it("lists the side the mode keeps, whichever side Windows is on", async () => {
    const { picker } = await opened(true, { ...THEMES, mode: "light" });
    expect(titles(picker)).toEqual(["VS Code Light Modern *", "Gruvbox Light Hard", "Dark themes…"]);
    expect(picker.start).toBe(0);
  });

  it("previews each theme it moves to on the whole window at once, and the theme in force on the last row", async () => {
    const { theme, picker } = await opened(true);
    picker.move(themeRow(picker, "VS Code Dark Modern"));
    expect(theme.shown?.name).toBe("VS Code Dark Modern");
    expect(theme.current?.name).toBe("Gruvbox Dark Hard");
    const last = picker.rows.at(-1);
    if (last) picker.move(last);
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
  });

  it("saves the theme picked to its side's setting, which stays in force", async () => {
    const { theme, picker } = await opened(true);
    picker.move(themeRow(picker, "VS Code Dark Modern"));
    await picker.pick(themeRow(picker, "VS Code Dark Modern"));
    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith("dark", "VS Code Dark Modern");
    expect(theme.current?.name).toBe("VS Code Dark Modern");
    expect(theme.preview).toBeUndefined();
  });

  it("puts the theme in force back when it closes without a pick", async () => {
    const { theme, picker } = await opened(true);
    picker.move(themeRow(picker, "VS Code Dark Modern"));
    picker.cancel();
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
    expect(commands.saveTheme).not.toHaveBeenCalled();
  });

  it("switches to the other side's list on its last row, opening on that side's choice", async () => {
    const { picker } = await opened(true);
    picker.switchSide();
    expect(picker.side).toBe("light");
    expect(titles(picker)).toEqual(["VS Code Light Modern *", "Gruvbox Light Hard", "Dark themes…"]);
    expect(picker.start).toBe(0);
    picker.switchSide();
    expect(picker.side).toBe("dark");
  });

  it("saves a pick from the other side's list to that side, and leaves the mode and the theme in force", async () => {
    const { theme, picker } = await opened(true);
    picker.switchSide();
    picker.move(themeRow(picker, "Gruvbox Light Hard"));
    expect(theme.shown?.name).toBe("Gruvbox Light Hard");

    await picker.pick(themeRow(picker, "Gruvbox Light Hard"));

    expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith("light", "Gruvbox Light Hard");
    expect(theme.chosen("light")).toBe("Gruvbox Light Hard");
    expect(theme.themes?.mode).toBe("system");
    expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
  });
});
