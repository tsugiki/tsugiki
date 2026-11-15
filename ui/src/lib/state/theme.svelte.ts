// The window's colours (#515). The core sends every theme of each side, light and dark, with the one chosen
// for each side and the theme in force (#755). Following the system, the window shows the chosen theme for
// the side Windows is on, jumping to the other when Windows changes sides. A theme the settings name shows
// whatever Windows says, and so does the chosen theme of the side a mode from a file written before keeps.
// The theme picker previews a theme over the one in force, and saves the theme in force (#518, #756). The
// Settings dialog shows its `[appearance]` on the window as the core works out the themes it gives, title
// bar included, and its Apply saves it (#757).
import {
  type AppearanceSettings,
  commands,
  type FailedThemeView,
  type ThemeInForce,
  type ThemeKind,
  type ThemeName,
  type ThemePick,
  type Themes,
  type ThemeView,
} from "$lib/data/bindings";
import { ask } from "./answer";

export type { AppearanceSettings, FailedThemeView, ThemeKind, Themes, ThemeView };

/** The key of `[appearance]` that holds the theme chosen for each side. */
export const SIDE_KEYS = { light: "light_theme", dark: "dark_theme" } as const satisfies Record<
  ThemeKind,
  keyof AppearanceSettings
>;

/** Whether `theme`, as `[appearance] theme` holds it, is a mode from a file written before (#755), which
 * shows the chosen theme of its side. Only a hand edit writes one. */
export const isMode = (theme: string): theme is ThemeKind => theme === "light" || theme === "dark";

/** The pick that saves `theme` to `[appearance] theme`: following the system, or the theme named. */
export const themePick = (theme: "system" | ThemeName): ThemePick =>
  theme === "system" ? { kind: "system" } : { kind: "named", name: theme };

/** Whether `one` and `other` hold the same `[appearance]`. */
const sameAppearance = (one: AppearanceSettings, other: AppearanceSettings) =>
  one.theme === other.theme && one.light_theme === other.light_theme && one.dark_theme === other.dark_theme;

/** The theme `themes` chose for side `kind`, which following the system shows on that side. */
const chosenIn = (themes: Themes | undefined, kind: ThemeKind): ThemeView | undefined => {
  const name = themes?.[kind].chosen;
  return themes?.[kind].all.find((one) => one.name === name);
};

/** The themes the core worked out for `values`, an `[appearance]` the Settings dialog shows. */
interface DialogThemes {
  themes: Themes;
  values: AppearanceSettings;
}

/** What the window shows of the `[appearance]` the Settings dialog shows (#757). The window's side follows
 * it: the side of `shown`'s themes while the dialog is on with some, or else the side of the theme in force,
 * which a load may turn. */
type DialogPreview =
  /** The dialog doesn't show its `[appearance]` on the window. */
  | { kind: "off" }
  /** The dialog has asked to show it, and the window shows `shown`, once the core has worked out some
   * themes, under the picker's preview and over the theme in force. */
  | { kind: "on"; shown?: DialogThemes }
  /** Apply saved values the window has no themes for: it goes on showing `shown` until its load answers. */
  | { kind: "ending"; shown: DialogThemes };

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
  /** Why the window can't show the `[appearance]` the Settings dialog shows, until it can or the dialog stops
   * showing it. */
  previewProblem = $state<string>();
  /** Why the last pick couldn't be saved, until one is. */
  problem = $state<string>();
  /** Why the window's title bar couldn't turn to the side of the theme in force, until a turn works. */
  turnProblem = $state<string>();
  /** How many picks have been made, so a load that crossed one keeps it, and a failed save puts the theme
   * back only if none came after it. */
  #picks = 0;
  /** The picks' saves, one after the other, so the file takes them, and they answer, in the order picked. A
   * load waits for those asked before it, so it reads what they wrote. It never rejects. */
  #saves = Promise.resolve<unknown>(undefined);
  /** The theme in force that `settings.toml` holds, as the last load that crossed no later pick read it, or
   * a save that worked since wrote it. A save that fails puts this back, not a pick whose save hasn't
   * answered. No pick comes before the first load. */
  #saved: ThemeInForce = { kind: "system" };
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
  /** The last load asked, which answers for the loads asked before it. It never rejects. */
  #latest = Promise.resolve<string | undefined>(undefined);
  /** What the window shows of the `[appearance]` the Settings dialog shows (#757). */
  #dialog = $state.raw<DialogPreview>({ kind: "off" });
  /** The values the dialog asked to show that no ask has taken yet: only the latest wait. */
  #wanted: AppearanceSettings | undefined;
  /** The dialog's asks on their way, one at a time, until none wait. It never rejects. */
  #showing: Promise<void> | undefined;
  /** The count of the dialog's changes, and of its stops, so an answer for values changed since doesn't
   * show. */
  #previews = 0;
  /** The side in use: the one the settings keep, or else the one Windows is on. */
  side: ThemeKind = $derived(this.#sideOf(this.themes));
  /** The theme in force, once the core has sent them: the named one, or else the chosen one of the side in
   * use. */
  current = $derived(this.#inForce(this.themes));
  /** The theme following the system shows: the chosen theme of the side Windows is on, as far as the page
   * knows. While a theme keeps the window on one side, the page hears that side, and learns Windows' own
   * only as the window turns back to it. */
  following = $derived(chosenIn(this.themes, this.dark ? "dark" : "light"));
  /** What the window shows: the picker's preview, or else the Settings dialog's, or else the theme in
   * force. */
  shown = $derived(this.preview ?? this.#inForce(this.#dialogShown()?.themes) ?? this.current);

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
   * don't come keep the page on the themes it had, and the window on their side. It asks once the saves
   * picked before it have answered. */
  async load(): Promise<void> {
    await this.#load();
  }

  /** Loads as `load` says, and gives why the core refused the load that showed, if it did: a load asked
   * later answers for this one. */
  #load(): Promise<string | undefined> {
    const loading = this.#loadAsked(++this.#loads);
    this.#latest = loading;
    return loading;
  }

  /** The load counted `asked`, as `#load` says. */
  async #loadAsked(asked: number): Promise<string | undefined> {
    const picks = this.#picks;
    await this.#saves;
    const answer = await ask(commands.themes());
    if (asked !== this.#loads) return this.#latest;
    if (answer.status !== "ok") {
      this.error = answer.error.message;
      // A load asked before this one may have turned the window before this one was asked.
      if (this.themes) this.#take(await this.#turnInForce(this.themes.theme));
      return answer.error.message;
    }
    const data = answer.data;
    // The core may have read the settings before the save of a pick made since, so the theme on the window
    // stands, and so it does if one comes while the window turns. Otherwise the answer says what the file
    // holds.
    const keepPicks = () => {
      if (this.#picks !== picks && this.themes) data.theme = $state.snapshot(this.themes.theme);
    };
    if (this.#picks === picks) this.#saved = data.theme;
    keepPicks();
    this.#take(await this.#turnInForce(data.theme));
    if (asked !== this.#loads) return this.#latest;
    keepPicks();
    this.themes = data;
    this.error = undefined;
    return undefined;
  }

  /** Shows the `[appearance]` the Settings dialog shows, `appearance`, on the window, as the core works out
   * the themes it gives, and turns the window to the side of the theme it shows (#757). Following the
   * system, the window then says the side the system is on. One ask goes at a time: values changed while
   * it's on its way wait, and only the latest are asked next, as each ask reads the `themes` folder. An
   * answer for values changed since doesn't show. One the core refuses leaves the window as it was, and
   * `previewProblem` says why. It answers once no values wait. */
  previewSettings(appearance: AppearanceSettings): Promise<void> {
    if (this.#dialog.kind === "off") this.#dialog = { kind: "on" };
    this.#previews += 1;
    this.#wanted = appearance;
    this.#showing ??= this.#showWanted();
    return this.#showing;
  }

  /** Asks for the values that wait until none do, and then clears `#showing`, in the step that finds none, so
   * values asked to show after it start the next round of asks. The window turns only for an answer for
   * values the dialog still shows. Once the asks stop, it goes to the side of what it shows, as an answer
   * changed while the window turned to it, or a load that turned it meanwhile, may have left it on another.
   */
  async #showWanted(): Promise<void> {
    let turned = false;
    for (;;) {
      const appearance = this.#wanted;
      if (appearance === undefined) {
        if (!turned || this.#dialog.kind !== "on") break;
        turned = false;
        await this.#align();
        continue;
      }
      this.#wanted = undefined;
      const asked = this.#previews;
      // A broken call answers as a refusal, so the asks go on.
      const answer = await ask<Themes>(Promise.resolve().then(() => commands.previewThemes(appearance)));
      if (asked !== this.#previews) continue;
      if (answer.status !== "ok") {
        this.previewProblem = `Tsugiki can't show the theme: ${answer.error.message}`;
        continue;
      }
      turned = true;
      this.#take(await this.#turn(answer.data.theme));
      if (asked !== this.#previews) continue;
      this.previewProblem = undefined;
      this.#dialog = { kind: "on", shown: { themes: answer.data, values: appearance } };
    }
    this.#showing = undefined;
  }

  /** Stops showing the Settings dialog's `[appearance]`. Once Apply has `saved` it, the window keeps the
   * themes the core worked out for those values, waiting for the ask on its way, and reads them again for
   * the files' problems. Without them it goes on showing what it shows until it has read them, and gives
   * why the core refused that read: the window then shows the theme in force from the last read that
   * worked, as after any read that doesn't, and the dialog has nothing more to put back. Otherwise the
   * window goes back to the theme in force and its side. */
  async endPreview(saved?: AppearanceSettings): Promise<string | undefined> {
    if (this.#dialog.kind === "off") return undefined;
    if (saved) await this.#showing;
    const shown = this.#dialogShown();
    this.#previews += 1;
    this.#wanted = undefined;
    this.previewProblem = undefined;
    if (!saved) {
      this.#dialog = { kind: "off" };
      await this.#align();
      return undefined;
    }
    if (shown && sameAppearance(shown.values, saved)) {
      this.#dialog = { kind: "off" };
      this.themes = shown.themes;
      this.#saved = shown.themes.theme;
      void this.load();
      return undefined;
    }
    this.#dialog = shown ? { kind: "ending", shown } : { kind: "off" };
    const refused = await this.#load();
    this.#dialog = { kind: "off" };
    return refused && `Tsugiki saved the settings, but can't show the theme: ${refused}`;
  }

  /** The themes the window shows for the Settings dialog, if it shows any. */
  #dialogShown(): DialogThemes | undefined {
    return this.#dialog.kind === "off" ? undefined : this.#dialog.shown;
  }

  /** Turns the window to the side of the themes it shows: the Settings dialog's, or else those in force. */
  async #align(): Promise<void> {
    const themes = this.#dialogShown()?.themes ?? this.themes;
    if (themes) this.#take(await this.#turn(themes.theme));
  }

  /** Turns the window to the side of the theme in force, as `#turn` does, unless the Settings dialog shows
   * themes for its `[appearance]`, which keep the window to their side. */
  #turnInForce(theme: ThemeInForce): Promise<ThemeKind | undefined> {
    const dialog = this.#dialog;
    return dialog.kind === "on" && dialog.shown ? Promise.resolve(undefined) : this.#turn(theme);
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

  /** The theme `themes` put in force: the named one, or else the chosen one of the side a mode keeps, or of
   * the side Windows is on. */
  #inForce(themes: Themes | undefined): ThemeView | undefined {
    const side = this.#sideOf(themes);
    if (themes?.theme.kind !== "named") return chosenIn(themes, side);
    const { name } = themes.theme;
    return themes[side].all.find((one) => one.name === name);
  }

  /** The side `themes` put in force: the one a named theme or a mode keeps, or else the one Windows is on. */
  #sideOf(themes: Themes | undefined): ThemeKind {
    const theme = themes?.theme;
    if (theme && theme.kind !== "system") return theme.side;
    return this.dark ? "dark" : "light";
  }

  /** Makes `theme` the theme in force, or follows the system, and saves it to `settings.toml` at once,
   * without Settings' Apply (#756). The window turns to the side it keeps. One that can't be saved puts the
   * theme the file holds back, unless the user has picked again since, and `problem` says why. Its save
   * goes once the saves picked before it have answered. */
  async pick(theme: ThemeView | "system"): Promise<void> {
    if (!this.themes) return;
    const pick = ++this.#picks;
    const inForce: ThemeInForce =
      theme === "system" ? { kind: "system" } : { kind: "named", name: theme.name, side: theme.kind };
    const saved = themePick(theme === "system" ? theme : theme.name);
    this.themes.theme = inForce;
    // The side the window says it turned to counts when it says it, as Windows may change sides after.
    const turned = this.#turn(inForce).then((side) => {
      this.#take(side);
    });
    // A save waits for the one before it, and a load for both. A save that doesn't answer would hold them,
    // but the settings thread writes a small file, and a drive slow enough to hold it holds the `themes`
    // folder beside it too, which every load reads. A broken call answers as a refusal, so the chain goes on.
    const saving = this.#saves.then(() => ask(Promise.resolve().then(() => commands.saveTheme(saved))));
    this.#saves = saving;
    const answer = await saving;
    const putBack = answer.status !== "ok" && this.#picks === pick;
    if (answer.status === "ok") {
      this.#saved = inForce;
      this.problem = undefined;
    } else {
      this.problem = `Tsugiki can't save the theme: ${answer.error.message}`;
    }
    // On the themes a reload brought since, not the ones the pick was made on. The turn back goes in the
    // queue at once, behind the pick's own turn.
    if (putBack) {
      this.themes.theme = this.#saved;
      this.#take(await this.#turn(this.#saved));
    }
    await turned;
  }
}

/** Sets `theme`'s tokens as CSS variables on `root`, and its kind as the root's `color-scheme`, so scroll
 * bars and form controls follow. Each theme sets every token, so the last theme's leave nothing behind. */
export const applyTheme = (root: HTMLElement, theme: ThemeView) => {
  for (const { property, value } of theme.tokens) root.style.setProperty(property, value);
  root.style.colorScheme = theme.kind;
};
