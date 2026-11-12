import type { AppearanceSettings, Themes, ThemeView } from "../src/lib/data/bindings.ts";

/** `settings.toml`'s `[appearance]` with nothing set. */
export const appearance: AppearanceSettings = {
  theme: "system",
  light_theme: "VS Code Light Modern",
  dark_theme: "VS Code Dark Modern",
};

/** The built-in themes cut down to the tokens the tests read. */
const LIGHT_MODERN: ThemeView = {
  name: "VS Code Light Modern",
  kind: "light",
  tokens: [
    { property: "--bg", value: "#ffffff" },
    { property: "--fg", value: "#3b3b3b" },
    { property: "--selection-active", value: "#e8e8e8" },
    { property: "--selection-inactive", value: "#e4e6f1" },
    { property: "--focus-border", value: "#005fb8" },
    { property: "--state-open", value: "#1a7f37" },
    { property: "--state-done", value: "#8250df" },
    { property: "--hl-keyword", value: "#0000ff" },
  ],
};

const DARK_MODERN: ThemeView = {
  name: "VS Code Dark Modern",
  kind: "dark",
  tokens: [
    { property: "--bg", value: "#1f1f1f" },
    { property: "--fg", value: "#cccccc" },
    { property: "--selection-active", value: "#04395e" },
    { property: "--selection-inactive", value: "#37373d" },
    { property: "--focus-border", value: "#0078d4" },
    { property: "--state-open", value: "#3fb950" },
    { property: "--state-done", value: "#ab7df8" },
    { property: "--hl-keyword", value: "#569cd6" },
  ],
};

const GRUVBOX_DARK: ThemeView = {
  name: "Gruvbox Dark Hard",
  kind: "dark",
  tokens: [
    { property: "--bg", value: "#1d2021" },
    { property: "--fg", value: "#ebdbb2" },
  ],
};

/** The themes with VS Code Modern's chosen for each side, and Gruvbox Dark Hard to pick. */
export const themes: Themes = {
  mode: "system",
  light: { chosen: LIGHT_MODERN.name, all: [LIGHT_MODERN] },
  dark: { chosen: DARK_MODERN.name, all: [DARK_MODERN, GRUVBOX_DARK] },
};
