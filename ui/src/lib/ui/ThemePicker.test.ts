import { fireEvent, render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { type DarkQuery, ThemeState, type Themes, type ThemeView } from "$lib/state/theme.svelte";
import ThemePicker from "./ThemePicker.svelte";

const commands = vi.hoisted(() => ({ themes: vi.fn(), saveTheme: vi.fn() }));
vi.mock("$lib/data/bindings", () => ({ commands }));

const view = (name: string, kind: "light" | "dark"): ThemeView => ({ name, kind, tokens: [] });

const THEMES: Themes = {
  mode: "system",
  light: {
    chosen: "VS Code Light Modern",
    all: [view("VS Code Light Modern", "light"), view("Gruvbox Light Hard", "light")],
    failed: [],
  },
  dark: {
    chosen: "VS Code Dark Modern",
    all: [view("VS Code Dark Modern", "dark"), view("Gruvbox Dark Hard", "dark")],
    failed: [],
  },
  problems: [],
};

/** The picker open over the core's themes, with Windows on its dark side. */
async function opened(themes: Themes = THEMES) {
  vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: structuredClone(themes) });
  const theme = new ThemeState({ matches: true, addEventListener: () => {} } as unknown as DarkQuery);
  await theme.load();
  const onclose = vi.fn();
  const onsaved = vi.fn();
  render(ThemePicker, { theme, onclose, onsaved });
  return { theme, onclose, onsaved, input: screen.getByRole("combobox", { name: "Theme" }) };
}

/** Each row's text, the mark on the same line as the name. */
const rows = () => screen.getAllByRole("option").map((row) => row.textContent.replace(/\s/g, " "));
const picked = () => screen.getByRole("option", { selected: true }).textContent.replace(/\s/g, " ");

beforeEach(() => {
  vi.mocked(commands.saveTheme).mockReset();
  vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
});

it("lists the side in use in the palette, on the theme in force, which it marks", async () => {
  await opened();
  expect(screen.getByRole("dialog", { name: "Theme picker" })).toBeTruthy();
  expect(screen.getByRole("listbox", { name: "Dark themes" })).toBeTruthy();
  expect(rows()).toEqual([
    "VS Code Dark Modern chosen",
    "Gruvbox Dark Hard",
    "Reload themes",
    "Light themes…",
  ]);
  expect(picked()).toBe("VS Code Dark Modern chosen");
});

it("shows a theme file that failed greyed with why, and Enter on it neither saves nor closes", async () => {
  const failed = { name: "Night", file: "night.toml", reason: "it extends Dusk, which no theme is named" };
  const { onclose, input } = await opened({ ...THEMES, dark: { ...THEMES.dark, failed: [failed] } });
  const row = screen.getByRole("option", { name: /Night/ });
  expect(row.getAttribute("aria-disabled")).toBe("true");
  expect(row.textContent).toBe("Night (night.toml) · it extends Dusk, which no theme is named");

  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(picked()).toBe("Night (night.toml) · it extends Dusk, which no theme is named");
  await fireEvent.keyDown(input, { key: "Enter" });

  expect(onclose).not.toHaveBeenCalled();
  expect(commands.saveTheme).not.toHaveBeenCalled();
});

it("reads the themes folder again on Reload themes and stays open on the new list", async () => {
  const { onclose } = await opened();
  vi.mocked(commands.themes).mockResolvedValue({
    status: "ok",
    data: { ...THEMES, dark: { ...THEMES.dark, all: [...THEMES.dark.all, view("Solarized Dark", "dark")] } },
  });
  await fireEvent.click(screen.getByRole("option", { name: "Reload themes" }));
  await vi.waitFor(() => {
    expect(rows()).toContain("Solarized Dark");
  });
  expect(commands.themes).toHaveBeenCalledTimes(2);
  expect(onclose).not.toHaveBeenCalled();
});

it("previews each theme the arrows move to, and the theme in force on the last row", async () => {
  const { theme, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(theme.shown?.name).toBe("VS Code Dark Modern");
  await fireEvent.keyDown(input, { key: "ArrowUp" });
  expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
});

it("saves the theme on Enter and closes, keeping it on the window, and says when it has saved", async () => {
  const { theme, onclose, onsaved, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "Enter" });
  await vi.waitFor(() => {
    expect(onsaved).toHaveBeenCalledOnce();
  });
  expect(onclose).toHaveBeenCalledOnce();
  expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith("dark", "Gruvbox Dark Hard");
  expect(theme.shown?.name).toBe("Gruvbox Dark Hard");
  expect(theme.preview).toBeUndefined();
});

it("puts the theme in force back on Esc, and saves nothing", async () => {
  const { theme, onclose, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(onclose).toHaveBeenCalledOnce();
  expect(theme.shown?.name).toBe("VS Code Dark Modern");
  expect(commands.saveTheme).not.toHaveBeenCalled();
});

it("switches to the other side's list on its last row, and a pick there leaves the window alone", async () => {
  const { theme, onclose, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "Enter" });

  expect(onclose).not.toHaveBeenCalled();
  expect(screen.getByRole("listbox", { name: "Light themes" })).toBeTruthy();
  expect(rows()).toEqual([
    "VS Code Light Modern chosen",
    "Gruvbox Light Hard",
    "Reload themes",
    "Dark themes…",
  ]);
  const box = screen.getByRole("combobox", { name: "Theme" });
  expect(document.activeElement).toBe(box);

  await fireEvent.keyDown(box, { key: "ArrowDown" });
  expect(theme.shown?.name).toBe("Gruvbox Light Hard");
  await fireEvent.keyDown(box, { key: "Enter" });
  await tick();

  expect(onclose).toHaveBeenCalledOnce();
  expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith("light", "Gruvbox Light Hard");
  expect(theme.chosen("light")).toBe("Gruvbox Light Hard");
  expect(theme.shown?.name).toBe("VS Code Dark Modern");
});
