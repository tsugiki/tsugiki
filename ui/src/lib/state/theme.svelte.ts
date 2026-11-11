// The window's colours (#515). The core sends a theme for each side, light and dark, and the window shows
// the one for the side Windows is on, jumping to the other when Windows changes sides.
import { commands, type Themes, type ThemeView } from "$lib/data/bindings";
import { ask } from "./answer";

export type { Themes, ThemeView };

/** Says which side Windows is on: `matchMedia("(prefers-color-scheme: dark)")` in the window. */
export type DarkQuery = Pick<MediaQueryList, "matches" | "addEventListener">;

export class ThemeState {
  themes = $state<Themes>();
  error = $state<string>();
  /** Whether Windows is on its dark side. */
  dark = $state(false);
  /** The theme in force, once the core has sent them. */
  current = $derived(this.themes && (this.dark ? this.themes.dark : this.themes.light));

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
