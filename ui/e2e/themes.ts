import type { Themes } from "../src/lib/data/bindings.ts";

/** The built-in themes cut down to the tokens the tests read, with VS Code Modern's values. */
export const themes: Themes = {
  light: {
    name: "VS Code Light Modern",
    kind: "light",
    tokens: [
      { property: "--bg", value: "#ffffff" },
      { property: "--fg", value: "#3b3b3b" },
      { property: "--state-open", value: "#1a7f37" },
      { property: "--state-done", value: "#8250df" },
      { property: "--hl-keyword", value: "#0000ff" },
    ],
  },
  dark: {
    name: "VS Code Dark Modern",
    kind: "dark",
    tokens: [
      { property: "--bg", value: "#1f1f1f" },
      { property: "--fg", value: "#cccccc" },
      { property: "--state-open", value: "#3fb950" },
      { property: "--state-done", value: "#ab7df8" },
      { property: "--hl-keyword", value: "#569cd6" },
    ],
  },
};
