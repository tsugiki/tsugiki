// The window's colours (#515). The core sends every theme of each side, light and dark, with the one chosen
// for each side and the mode (#517). In System mode the window shows the chosen theme for the side Windows
// is on, jumping to the other when Windows changes sides. Light and Dark show their side's theme whatever
// Windows says. The theme picker previews a theme over the one in force, and saves a side's choice (#518).
import { commands, type ThemeKind, type Themes, type ThemeView } from "$lib/data/bindings";
import { ask } from "./answer";

export type { ThemeKind, Themes, ThemeView };

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
  /** The side in use: the one Windows is on in System mode, and otherwise the mode's. */
  side: ThemeKind = $derived.by(() => {
    const mode = this.themes?.mode ?? "system";
    if (mode !== "system") return mode;
    return this.dark ? "dark" : "light";
  });
  /** The theme in force, once the core has sent them: the chosen one of the side in use. */
  current = $derived(this.themesOf(this.side).find((theme) => theme.name === this.chosen(this.side)));
  /** What the window shows: the preview, or else the theme in force. */
  shown = $derived(this.preview ?? this.current);

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

  /** Every theme of side `kind`, in the order the picker lists them. */
  themesOf(kind: ThemeKind): readonly ThemeView[] {
    return this.themes?.[kind].all ?? [];
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
    const side = this.themes[kind];
    const before = side.chosen;
    side.chosen = name;
    const answer = await ask(commands.saveTheme(kind, name));
    if (answer.status === "ok") {
      this.problem = undefined;
    } else {
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
