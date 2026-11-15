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
  /** Why the window's title bar couldn't turn to the side of the theme in force, until a turn works. */
  turnProblem = $state<string>();
  /** How many choices have been made, so a load that crossed one keeps it. */
  #choices = 0;
  /** How many saves are on their way, which a load may have read the settings before. */
  #saving = 0;
  /** The count of loads asked, so only the last one asked shows: one asked later read settings no older. */
  #loads = 0;
  /** The window's turns, one after the other, so each reaches the window after the one before it. */
  #turning = Promise.resolve<unknown>(undefined);
  /** The side the last turn asked for, `null` to follow Windows, or `undefined` when the page doesn't know
   * the window's side: before the first turn, as the window may have started on a side the settings keep,
   * and after a turn that failed. */
  #asked: ThemeKind | null | undefined = undefined;
  /** The media query's `matches` for the side kept before a turn back to Windows, which the webview may
   * report late. An event that reports the other side can't be the webview catching up. */
  #kept: boolean | undefined;
  /** The count of media events that report Windows' side, so a side the window said before one doesn't undo
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
      if (e.matches !== this.#kept) this.#heard += 1;
      this.dark = e.matches;
    });
  }

  /** What is wrong with the user's theme files, in words for the status bar (#519). */
  problems: readonly string[] = $derived(this.themes?.problems ?? []);

  /** Asks the core for the themes, turns the window to the side of the theme in force, and then shows them,
   * so the page shows the theme with the side the window said. The core reads the `themes` folder each
   * time, so this is also Reload themes (#519). Of loads that cross, the one asked last shows. Themes that
   * don't come keep the page on the themes it had, and the window on their side. */
  async load() {
    const asked = ++this.#loads;
    const choices = this.#choices;
    const answer = await ask(commands.themes());
    if (asked !== this.#loads) return;
    if (answer.status !== "ok") {
      this.error = answer.error.message;
      // A load asked before this one may have turned the window before this one was asked.
      if (this.themes) this.#take(await this.#turn(this.themes.theme));
      return;
    }
    this.#take(await this.#turn(answer.data.theme));
    if (asked !== this.#loads) return;
    // The core may have read the settings before a choice made since, or before a save that is on its way,
    // so the choices on the window stand.
    if (this.themes && (this.#choices !== choices || this.#saving > 0)) {
      answer.data.light.chosen = this.themes.light.chosen;
      answer.data.dark.chosen = this.themes.dark.chosen;
    }
    this.themes = answer.data;
    this.error = undefined;
  }

  /** Takes `side` as the one Windows is on, when the window said it. */
  #take(side: ThemeKind | undefined) {
    if (side) this.dark = side === "dark";
  }

  /** Turns the window to the side `theme` keeps, its title bar with it, or back to following Windows, once
   * the turns asked before have answered. Back to Windows, it gives the side the window said it is on. */
  #turn(theme: ThemeInForce): Promise<ThemeKind | undefined> {
    const side = theme.kind === "system" ? null : theme.side;
    const turned = this.#turning.then(() => this.#turnNow(side));
    this.#turning = turned;
    return turned;
  }

  /** Turns the window to `side`, unless the last turn asked for it already. Back to Windows, the window says
   * the side it is on, which the media query hears of only a moment later, as a theme that kept the window
   * on one side kept the query there too. The side it said is given unless the query has heard Windows
   * change sides since. */
  async #turnNow(side: ThemeKind | null): Promise<ThemeKind | undefined> {
    const before = this.#asked;
    if (side === before) return undefined;
    this.#asked = side;
    this.#kept = side === null && before ? before === "dark" : undefined;
    const heard = this.#heard;
    try {
      const said = await commands.turnWindow(side);
      this.turnProblem = undefined;
      return said !== null && this.#heard === heard ? said : undefined;
    } catch (e) {
      // The window may not have turned, so the next turn goes whatever it asks for.
      this.#asked = undefined;
      this.turnProblem = `Tsugiki can't turn its title bar to the theme's side: ${String(e)}`;
      return undefined;
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
