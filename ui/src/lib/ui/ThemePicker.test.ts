import { fireEvent, render, screen, within } from "@testing-library/svelte";
import { beforeEach, expect, it, vi } from "vitest";
import { type DarkQuery, ThemeState, type Themes } from "$lib/state/theme.svelte";
import { builtInThemes, themeView } from "$lib/testing/fixtures";
import ThemePicker from "./ThemePicker.svelte";

const commands = vi.hoisted(() => ({
  themes: vi.fn(),
  saveTheme: vi.fn(),
  turnWindow: vi.fn(async () => null),
}));
vi.mock("$lib/data/bindings", () => ({ commands }));

const THEMES: Themes = builtInThemes("VS Code Dark Modern");

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

const text = (row: HTMLElement) => row.textContent.replace(/\s/g, " ");
/** Each row's text, the mark on the same line as the name. */
const rows = () => screen.getAllByRole("option").map(text);
const picked = () => text(screen.getByRole("option", { selected: true }));
/** The rows of the group named `name`. */
const groupRows = (name: string) =>
  within(screen.getByRole("group", { name })).getAllByRole("option").map(text);

beforeEach(() => {
  vi.mocked(commands.saveTheme).mockReset();
  vi.mocked(commands.saveTheme).mockResolvedValue({ status: "ok", data: null });
});

it("lists Follow system, then each side's themes under its heading, and opens on the row in force", async () => {
  const failed = { name: "Night", file: "night.toml", reason: "it extends Dusk, which no theme is named" };
  await opened({ ...THEMES, dark: { ...THEMES.dark, failed: [failed] } });
  expect(screen.getByRole("dialog", { name: "Theme picker" })).toBeTruthy();
  expect(screen.getByRole("listbox", { name: "Themes" })).toBeTruthy();
  expect(rows()).toEqual([
    "Follow system chosen",
    "VS Code Light Modern",
    "Gruvbox Light Hard",
    "VS Code Dark Modern",
    "Gruvbox Dark Hard",
    "Night (night.toml) · it extends Dusk, which no theme is named",
    "Reload themes",
  ]);
  expect(groupRows("Light")).toEqual(["VS Code Light Modern", "Gruvbox Light Hard"]);
  expect(groupRows("Dark")).toEqual(["VS Code Dark Modern", "Gruvbox Dark Hard"]);
  expect(screen.getAllByRole("group")).toHaveLength(2);
  expect(picked()).toBe("Follow system chosen");
});

it("opens on the theme the settings name, which it marks", async () => {
  await opened({ ...THEMES, theme: { kind: "named", name: "Gruvbox Light Hard", side: "light" } });
  expect(picked()).toBe("Gruvbox Light Hard chosen");
  expect(rows().filter((row) => row.endsWith("chosen"))).toEqual(["Gruvbox Light Hard chosen"]);
});

it("drops a heading whose themes the text doesn't match", async () => {
  const { input } = await opened();
  await fireEvent.input(input, { target: { value: "gruvbox d" } });
  expect(rows()).toEqual(["Gruvbox Dark Hard"]);
  expect(screen.getAllByRole("group").map((group) => group.getAttribute("aria-label"))).toEqual(["Dark"]);
});

it("shows a theme file that failed greyed with why, and Enter on it neither saves nor closes", async () => {
  const failed = { name: "Night", file: "night.toml", reason: "it extends Dusk, which no theme is named" };
  const { onclose, input } = await opened({ ...THEMES, dark: { ...THEMES.dark, failed: [failed] } });
  const row = screen.getByRole("option", { name: /Night/ });
  expect(row.getAttribute("aria-disabled")).toBe("true");

  for (let i = 0; i < 5; i++) await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(picked()).toBe("Night (night.toml) · it extends Dusk, which no theme is named");
  await fireEvent.keyDown(input, { key: "Enter" });

  expect(onclose).not.toHaveBeenCalled();
  expect(commands.saveTheme).not.toHaveBeenCalled();
});

it("reads the themes folder again on Reload themes and stays open on the new list", async () => {
  const { onclose } = await opened();
  vi.mocked(commands.themes).mockResolvedValue({
    status: "ok",
    data: {
      ...THEMES,
      dark: { ...THEMES.dark, all: [...THEMES.dark.all, themeView("Solarized Dark", "dark")] },
    },
  });
  await fireEvent.click(screen.getByRole("option", { name: "Reload themes" }));
  await vi.waitFor(() => {
    expect(groupRows("Dark")).toContain("Solarized Dark");
  });
  expect(commands.themes).toHaveBeenCalledTimes(2);
  expect(onclose).not.toHaveBeenCalled();
});

it("previews each theme the arrows move to, and the system's on Follow system", async () => {
  const { theme, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(theme.shown?.name).toBe("VS Code Light Modern");
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  expect(theme.shown?.name).toBe("Gruvbox Light Hard");
  await fireEvent.keyDown(input, { key: "ArrowUp" });
  await fireEvent.keyDown(input, { key: "ArrowUp" });
  expect(theme.shown?.name).toBe("VS Code Dark Modern");
});

it("saves a light theme as the theme in force on Enter while Windows is dark, and says when it has saved", async () => {
  const { theme, onclose, onsaved, input } = await opened();
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "ArrowDown" });
  await fireEvent.keyDown(input, { key: "Enter" });
  await vi.waitFor(() => {
    expect(onsaved).toHaveBeenCalledOnce();
  });
  expect(onclose).toHaveBeenCalledOnce();
  expect(commands.saveTheme).toHaveBeenCalledExactlyOnceWith({ kind: "named", name: "Gruvbox Light Hard" });
  expect(theme.shown?.name).toBe("Gruvbox Light Hard");
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
