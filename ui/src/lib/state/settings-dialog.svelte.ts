// What the Settings dialog (docs/devel/settings.md) does with the settings: it reads the files again as it
// opens, previews each change on the window at once, and Apply saves them or Cancel puts them back (#520).
import {
  type AppearanceChanges_Deserialize,
  type AppearanceSettings,
  commands,
  type SettingsChanges_Deserialize,
} from "$lib/data/bindings";
import { type Answer, ask } from "./answer";
import type { LanguageServerSettings, LoadedSettings, Settings, SettingsState } from "./settings.svelte";
import { isMode, SIDE_KEYS, type ThemeKind, type ThemeState, themePick } from "./theme.svelte";

/** The pages of the dialog's left column, in its order. */
export const SETTINGS_PAGES = ["General", "Account", "Repositories", "Sync", "Keyboard", "Advanced"] as const;
export type SettingsPage = (typeof SETTINGS_PAGES)[number];
/** What the dialog shows on its right: one of its pages, or a Repository's page under Repositories. */
export type Shown = { page: SettingsPage } | { repository: string };
/** Where a setting is stored, which its row says (ADR 0014): `settings.toml`, `machine.toml`, the database or
 * the OS keychain. */
export type StoredIn = "settings" | "machine" | "database" | "keychain";
/** The tables of `settings.toml` the dialog edits. */
type Edited = "status_bar" | "releases";

/** What the files said as the dialog opened, or as its last Apply saved them. */
interface Saved {
  settings: Settings;
  servers: LanguageServerSettings;
}

/** The keys of `[appearance]` that differ in `now` from `then`, which Apply saves, or nothing if none do. A
 * key the dialog left keeps what the file says, such as a mode from before or a name no Theme has. A mode
 * never goes: the Theme row offers only the file's own, which is no change, and the core refuses one. */
const appearanceChanges = (
  now: AppearanceSettings,
  then: AppearanceSettings,
): AppearanceChanges_Deserialize | undefined => {
  const changes: AppearanceChanges_Deserialize = {
    ...(now.theme !== then.theme && { theme: themePick(now.theme) }),
    ...(now.light_theme !== then.light_theme && { light_theme: now.light_theme }),
    ...(now.dark_theme !== then.dark_theme && { dark_theme: now.dark_theme }),
  };
  return Object.keys(changes).length > 0 ? changes : undefined;
};

export class SettingsDialog {
  readonly #settings: SettingsState;
  /** The window's themes, which show the dialog's `[appearance]` while it's open (#757). */
  readonly #theme: ThemeState;
  shown = $state<Shown>({ page: "General" });
  /** What the files said once the dialog read them, which Cancel puts back. Unset until then, and for good if
   * it couldn't read them. */
  #before = $state.raw<Saved>();
  /** Why the dialog couldn't read the files again or save to them, until the next try. */
  problem = $state<string>();
  /** Whether Apply's save is on its way, which Cancel can't take back. */
  applying = $state(false);

  /** The dialog previews on `settings`, and its `[appearance]` on `theme`, so a load mustn't replace the
   * settings until it closes. It opens on `shown`. */
  constructor(settings: SettingsState, theme: ThemeState, shown: Shown = { page: "General" }) {
    this.#settings = settings;
    this.#theme = theme;
    this.shown = shown;
    settings.preview(true);
  }

  /** The settings the dialog shows and changes, once it has read the files. */
  get current(): Settings | undefined {
    return this.#before === undefined ? undefined : this.#settings.settings;
  }

  /** The language servers' settings the dialog shows and changes, once it has read the files. */
  get servers(): LanguageServerSettings | undefined {
    return this.#before === undefined ? undefined : this.#settings.languageServers;
  }

  /** Each table the dialog edits that differs from what the files said as it opened, which Apply saves. */
  get changes(): SettingsChanges_Deserialize {
    const { current, servers } = this;
    const before = this.#before;
    if (current === undefined || servers === undefined || before === undefined) return {};
    const same = (now: unknown, then: unknown) => JSON.stringify(now) === JSON.stringify(then);
    const differs = (table: Edited) => !same(current[table], before.settings[table]);
    const appearance = appearanceChanges(current.appearance, before.settings.appearance);
    return {
      ...(differs("status_bar") && { status_bar: current.status_bar }),
      ...(differs("releases") && { releases: current.releases }),
      ...(appearance && { appearance }),
      ...(!same(servers, before.servers) && { language_servers: servers }),
    };
  }

  /** Whether anything changed, so the button reads Apply, not OK. */
  get changed(): boolean {
    return Object.keys(this.changes).length > 0;
  }

  /** The names of the Repositories, each with a page under Repositories. */
  get repositories(): string[] {
    return this.#settings.settings?.repositories.map((repository) => repository.name) ?? [];
  }

  /** Reads both files again, so a hand edit made since start-up shows and takes effect (ADR 0014), and says
   * whether it did. If the core can't, the dialog says why and its controls stay off. */
  async open(): Promise<boolean> {
    const answer = await ask(commands.rereadSettings());
    if (answer.status === "error") {
      this.problem = `Tsugiki can't read the settings files again: ${answer.error.message}`;
      return false;
    }
    this.#take(answer.data);
    return true;
  }

  /** Changes the settings in force, which the window shows at once. */
  change(edit: (settings: Settings) => void) {
    if (this.current !== undefined) edit(this.current);
  }

  /** The names of the themes of side `side` that can be used, in the order the theme picker lists them,
   * which Settings › General offers. Unset until the window has its themes. */
  themeNames(side: ThemeKind): string[] | undefined {
    if (this.#theme.themes === undefined) return undefined;
    return this.#theme.themesOf(side).map((theme) => theme.name);
  }

  /** The value of `row`'s key as the file held it when the dialog read it, with the words for its option,
   * when the row doesn't offer it otherwise: a mode from before in the Theme row, or a name no theme the
   * row lists has. It stays among the row's options after a change, so the user can go back to it, and Apply
   * over another change leaves it in the file. Unset when the row offers it, or before the files are read.
   * While the window waits for its themes, it shows as it is. */
  held(row: "theme" | ThemeKind): { value: string; words: string } | undefined {
    const value = this.#before?.settings.appearance[row === "theme" ? row : SIDE_KEYS[row]];
    if (value === undefined || (row === "theme" && value === "system")) return undefined;
    const themes = this.#theme.themes;
    if (themes === undefined) return { value, words: value };
    const names = (side: ThemeKind) => themes[side].all.some((theme) => theme.name === value);
    if (row === "theme") {
      if (names("light") || names("dark")) return undefined;
      if (isMode(value)) return { value, words: `Always the ${value === "light" ? "Light" : "Dark"} theme` };
    } else {
      if (names(row)) return undefined;
      const other = row === "light" ? "dark" : "light";
      if (names(other)) return { value, words: `${value} (a ${other} theme)` };
    }
    const failed = [...themes.light.failed, ...themes.dark.failed].some((theme) => theme.name === value);
    return { value, words: `${value} (${failed ? "can't be used" : "not found"})` };
  }

  /** Why the Appearance rows can't list the themes, or the window can't show what they say, until it can. */
  get themeProblem(): string | undefined {
    const theme = this.#theme;
    if (theme.themes === undefined && theme.error !== undefined) {
      return `Tsugiki can't list the themes: ${theme.error}`;
    }
    return theme.previewProblem;
  }

  /** Changes `[appearance]`, which the window shows once the core has worked out the themes it gives. */
  changeAppearance(edit: (appearance: AppearanceSettings) => void) {
    const current = this.current;
    if (current === undefined) return;
    edit(current.appearance);
    void this.#theme.previewSettings($state.snapshot(current.appearance));
  }

  /** Changes the language servers' settings, which take effect on Apply. */
  changeServers(edit: (servers: LanguageServerSettings) => void) {
    if (this.servers !== undefined) edit(this.servers);
  }

  /** Puts back what the files said as the dialog opened, and says whether it may close: not while Apply
   * saves. */
  cancel(): boolean {
    if (this.applying) return false;
    if (this.#before !== undefined) {
      this.#settings.settings = structuredClone(this.#before.settings);
      this.#settings.languageServers = structuredClone(this.#before.servers);
    }
    return this.#close();
  }

  /** Saves what changed, and says whether the dialog may close: with nothing changed it closes at once, as
   * OK, and a save the core refused keeps it open with why. A save that changed a language server keeps it
   * open too, so its row in Settings › Advanced can say what the core's check of it finds (#683). A save
   * that changed `[appearance]` makes the themes the window shows for it the themes in force, so the window
   * doesn't go back to the theme from before. If the window has none to show, it waits for them, and one
   * that can't get them keeps the dialog open with why. */
  async apply(): Promise<boolean> {
    if (this.applying) return false;
    const changes = $state.snapshot(this.changes);
    if (Object.keys(changes).length === 0) return this.#close();
    // The values Apply saves, whose themes the window keeps: the rows are off until it's done.
    const current = this.current;
    const saved = changes.appearance && current ? $state.snapshot(current.appearance) : undefined;
    this.applying = true;
    let answer: Answer<LoadedSettings>;
    let unshown: string | undefined;
    try {
      // A broken call answers as a refusal, so Cancel and Esc work again after it.
      answer = await ask(() => commands.applySettings(changes));
      unshown = answer.status === "ok" ? await this.#theme.endPreview(saved) : undefined;
    } finally {
      this.applying = false;
    }
    if (answer.status === "error") {
      this.problem = `Tsugiki can't save the settings: ${answer.error.message}`;
      return false;
    }
    if (unshown !== undefined) {
      this.problem = unshown;
      this.#take(answer.data);
      return false;
    }
    if (changes.language_servers === undefined) {
      this.#settings.take(answer.data);
      return this.#close();
    }
    this.problem = undefined;
    this.#take(answer.data);
    return false;
  }

  /** Holds what the files say now, `loaded`, which Cancel puts back and the changes are measured from. */
  #take(loaded: LoadedSettings) {
    this.#before = {
      settings: structuredClone(loaded.settings),
      servers: structuredClone(loaded.language_servers),
    };
    this.#settings.take(loaded);
  }

  /** Lets loads replace the settings again, and puts the theme in force back on the window, unless Apply
   * made the dialog's the theme in force. */
  #close(): true {
    this.#settings.preview(false);
    void this.#theme.endPreview();
    return true;
  }
}
