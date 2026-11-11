import { describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { applyTheme, type DarkQuery, ThemeState, type Themes } from "./theme.svelte";

vi.mock("$lib/data/bindings", () => ({ commands: { themes: vi.fn() } }));

const THEMES: Themes = {
  light: {
    name: "VS Code Light Modern",
    kind: "light",
    tokens: [
      { property: "--bg", value: "#ffffff" },
      { property: "--hl-keyword", value: "#0000ff" },
    ],
  },
  dark: {
    name: "VS Code Dark Modern",
    kind: "dark",
    tokens: [
      { property: "--bg", value: "#1f1f1f" },
      { property: "--hl-keyword", value: "#569cd6" },
    ],
  },
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

describe("ThemeState", () => {
  it("shows the theme of the side Windows is on once the core sent them", async () => {
    vi.mocked(commands.themes).mockResolvedValue({ status: "ok", data: THEMES });
    const state = new ThemeState(sides(true).query);
    expect(state.current).toBeUndefined();
    await state.load();
    expect(state.current?.name).toBe("VS Code Dark Modern");
    expect(state.error).toBeUndefined();
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

  it("holds the core's refusal and shows no theme", async () => {
    vi.mocked(commands.themes).mockResolvedValue({
      status: "error",
      error: { kind: "internal", message: "A built-in theme can't be read" },
    });
    const state = new ThemeState(sides(false).query);
    await state.load();
    expect(state.current).toBeUndefined();
    expect(state.error).toBe("A built-in theme can't be read");
  });
});

describe("applyTheme", () => {
  it("sets each token on the root and the theme's kind as its color-scheme", () => {
    const root = document.createElement("div");
    applyTheme(root, THEMES.dark);
    expect(root.style.getPropertyValue("--bg")).toBe("#1f1f1f");
    expect(root.style.getPropertyValue("--hl-keyword")).toBe("#569cd6");
    expect(root.style.colorScheme).toBe("dark");
    applyTheme(root, THEMES.light);
    expect(root.style.getPropertyValue("--bg")).toBe("#ffffff");
    expect(root.style.colorScheme).toBe("light");
  });
});
