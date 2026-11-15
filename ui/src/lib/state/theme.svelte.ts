// The window's colours (#515). The core sends every theme of each side, light and dark, with the one chosen
// for each side and the theme in force (#755). Following the system, the window shows the chosen theme for
// the side Windows is on, jumping to the other when Windows changes sides. A theme the settings name shows
// whatever Windows says, and so does the chosen theme of the side a mode from a file written before keeps.
// The theme picker previews a theme over the one in force, and saves a side's choice (#518).
import {
  commands,
  type FailedThemeView,
  type ThemeInForce,
  type ThemeKind,
  type Themes,
  type ThemeView,
} from "$lib/data/bindings";
import { ask } from "./answer";

export type { FailedThemeView, ThemeKind, Themes, ThemeView };

/** Says which side Windows is on: `matchMedia("(prefers-color-scheme: dark)")` in the window. */
export type DarkQuery = Pick<MediaQueryList, "matches" | "addEventListener">;

export class ThemeState {
  themes = $state<Themes>();
  /** Why the core sent no themes. The window then keeps the system's colours, which `app.html` sets. */
  error = $state<string>();
  /** Whether Windows is on its dark side. */
  dark = $state(false);
  /** A theme the picker shows on the window for now, over the one in force. */
  preview = $state<ThemeView>();
  /** Why the last choice couldn't be saved, until one is. */
  problem = $state<string>();
  /** How many choices have been made, so a load that crossed one keeps it. */
  #choices = 0;
  /** How many saves are on their way, which a load may have read the settings before. */
  #saving = 0;
  /** The count of side changes the media query has heard, so a side the window said before one doesn't undo
   * it. */
  #heard = 0;
  /** The side in use: the one the settings keep, or else the one Windows is on. */
  side: ThemeKind = $derived.by(() => {
    const theme = this.themes?.theme;
    if (theme && theme.kind !== "system") return theme.side;
    return this.dark ? "dark" : "light";
  });
  /** The theme in force, once the core has sent them: the named one, or else the chosen one of the side in
   * use. */
  current = $derived.by(() => {
    const theme = this.themes?.theme;
    const name = theme?.kind === "named" ? theme.name : this.chosen(this.side);
    return this.themesOf(this.side).find((one) => one.name === name);
  });
  /** What the window shows: the preview, or else the theme in force. */
  shown = $derived(this.preview ?? this.current);

  constructor(query: DarkQuery) {
    this.dark = query.matches;
    query.addEventListener("change", (e) => {
      this.#heard += 1;
      this.dark = e.matches;
    });
  }

  /** What is wrong with the user's theme files, in words for the status bar (#519). */
  problems: readonly string[] = $derived(this.themes?.problems ?? []);

  /** Asks the core for the themes, then turns the window to the side of the theme in force. The core reads
   * the `themes` folder each time, so this is also Reload themes (#519). Themes that don't come keep the
   * window on the side it was on, as the page keeps the themes it had. */
  async load() {
    const choices = this.#choices;
    const answer = await ask(commands.themes());
    if (answer.status === "ok") {
      // The core may have read the settings before a choice made since, or before a save that is on its
      // way, so the choices on the window stand.
      if (this.themes && (this.#choices !== choices || this.#saving > 0)) {
        answer.data.light.chosen = this.themes.light.chosen;
        answer.data.dark.chosen = this.themes.dark.chosen;
      }
      this.themes = answer.data;
      this.error = undefined;
      await this.#turn(answer.data.theme);
    } else {
      this.error = answer.error.message;
    }
  }

  /** Turns the window to the side `theme` keeps, its title bar with it, or back to following Windows. The
   * page asks after each answer it takes, so the window ends on the side of the last, as the page does.
   * Back to Windows, the window says the side it is on, which the media query hears of only a moment later:
   * a theme that kept the window on one side kept the query there too. */
  async #turn(theme: ThemeInForce) {
    const heard = this.#heard;
    try {
      const side = await commands.turnWindow(theme.kind === "system" ? null : theme.side);
      if (side && this.#heard === heard) this.dark = side === "dark";
    } catch {
      // A window that can't be reached keeps its side, and the page goes on with its media query.
    }
  }

  /** Every theme of side `kind`, in the order the picker lists them. */
  themesOf(kind: ThemeKind): readonly ThemeView[] {
    return this.themes?.[kind].all ?? [];
  }

  /** The theme files of side `kind` that can't be used, in name order. */
  failedOf(kind: ThemeKind): readonly FailedThemeView[] {
    return this.themes?.[kind].failed ?? [];
  }

  /** The name of the theme chosen for side `kind`. */
  chosen(kind: ThemeKind): string | undefined {
    return this.themes?.[kind].chosen;
  }

  /** Chooses the theme named `name` for side `kind`, and saves it to `settings.toml` at once, without
   * Settings' Apply. One that can't be saved puts the old choice back, unless the user has chosen again
   * since, and `problem` says why. */
  async choose(kind: ThemeKind, name: string): Promise<void> {
    if (!this.themes) return;
    this.#choices += 1;
    this.#saving += 1;
    const before = this.themes[kind].chosen;
    this.themes[kind].chosen = name;
    const answer = await ask(commands.saveTheme(kind, name));
    this.#saving -= 1;
    if (answer.status === "ok") {
      this.problem = undefined;
    } else {
      // The themes a reload brought since, not the ones the choice was made on.
      const side = this.themes[kind];
      if (side.chosen === name) side.chosen = before;
      this.problem = `Tsugiki can't save the theme: ${answer.error.message}`;
    }
  }
}

/** Sets `theme`'s tokens as CSS variables on `root`, and its kind as the root's `color-scheme`, so scroll
 * bars and form controls follow. Each theme sets every token, so the last theme's leave nothing behind. */
export const applyTheme = (root: HTMLElement, theme: ThemeView) => {
  for (const { property, value } of theme.tokens) root.style.setProperty(property, value);
  root.style.colorScheme = theme.kind;
};
