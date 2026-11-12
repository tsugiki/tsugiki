// The window's colours (#515). The core sends a theme for each side, light and dark, and the mode (#517). In
// System mode the window shows the theme for the side Windows is on, jumping to the other when Windows
// changes sides. Light and Dark show their side's theme whatever Windows says.
import { commands, type Themes, type ThemeView } from "$lib/data/bindings";
import { ask } from "./answer";

export type { Themes, ThemeView };

/** Says which side Windows is on: `matchMedia("(prefers-color-scheme: dark)")` in the window. */
export type DarkQuery = Pick<MediaQueryList, "matches" | "addEventListener">;

export class ThemeState {
  themes = $state<Themes>();
  /** Why the core sent no themes. The window then keeps the system's colours, which `app.html` sets. */
  error = $state<string>();
  /** Whether Windows is on its dark side. */
  dark = $state(false);
  /** The theme in force, once the core has sent them. */
  current = $derived.by(() => {
    if (!this.themes) return undefined;
    const { mode, light, dark } = this.themes;
    const darkSide = mode === "system" ? this.dark : mode === "dark";
    return darkSide ? dark : light;
  });

  constructor(query: DarkQuery) {
    this.dark = query.matches;
    query.addEventListener("change", (e) => {
      this.dark = e.matches;
    });
  }

  async load() {
    const answer = await ask(commands.themes());
    if (answer.status === "ok") this.themes = answer.data;
    else this.error = answer.error.message;
  }
}

/** Sets `theme`'s tokens as CSS variables on `root`, and its kind as the root's `color-scheme`, so scroll
 * bars and form controls follow. Each theme sets every token, so the last theme's leave nothing behind. */
export const applyTheme = (root: HTMLElement, theme: ThemeView) => {
  for (const { property, value } of theme.tokens) root.style.setProperty(property, value);
  root.style.colorScheme = theme.kind;
};
