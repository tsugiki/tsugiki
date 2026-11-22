// What the Settings dialog (docs/devel/settings.md) does with the settings: it reads the files again as it
// opens, previews each change on the window at once, and Apply saves them or Cancel puts them back (#520).
import {
  type AppearanceChanges_Deserialize,
  type AppearanceSettings,
  commands,
  type OnThisMachine_Serialize,
  type RepositorySettings_Serialize,
  type SettingsChanges_Deserialize,
} from "$lib/data/bindings";
import { type Answer, ask } from "./answer";
import { FilterEdits } from "./filter-edits.svelte";
import type { Home } from "./home.svelte";
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
/** A Repository as `settings.toml` holds it. */
export type RepositorySettings = RepositorySettings_Serialize;
/** What a Repository has on this machine: where its mirror gets its objects, and the mirror's size (#533). */
export type OnThisMachine = OnThisMachine_Serialize;
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
  /** What each Repository has on this machine, as the core last measured it. */
  #machine = $state.raw<OnThisMachine[]>();
  /** Why the core couldn't say what the Repositories have on this machine, until it next can. */
  machineProblem = $state<string>();
  /** How many times the dialog asked what the Repositories have on this machine. */
  #measures = 0;
  /** The read of the files as the dialog opens, which a measure waits for, so the core measures the
   * Repositories the files list now. */
  #reading: Promise<unknown> = Promise.resolve();
  /** Whether Add repository… added a Repository while the dialog was open. */
  #added = false;
  /** The Filters each Repository's page edits, which Apply saves, and which the window shows only once
   * saved, as the core reads each Filter's items (#535). */
  readonly filters = new FilterEdits();

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
    const filters = this.filters.changes;
    return {
      ...(differs("status_bar") && { status_bar: current.status_bar }),
      ...(differs("releases") && { releases: current.releases }),
      ...(appearance && { appearance }),
      ...(!same(servers, before.servers) && { language_servers: servers }),
      ...(filters.length > 0 && { filters }),
    };
  }

  /** Whether anything changed, so the button reads Apply, not OK. */
  get changed(): boolean {
    return Object.keys(this.changes).length > 0;
  }

  /** The reason a change can't be saved as it stands, as a Filter with a blank name or search, which keeps
   * Apply off, or nothing if each can. */
  get unsaveable(): string | undefined {
    const repository = this.filters.blankIn;
    return repository === undefined ? undefined : `Give each Filter of ${repository} a name and a search.`;
  }

  /** The names of the Repositories, each with a page under Repositories. */
  get repositories(): string[] {
    return this.listed.map((repository) => repository.name);
  }

  /** The Repositories the settings list, in their order, which Settings › Repositories shows (#533). */
  get listed(): RepositorySettings[] {
    return this.#settings.settings?.repositories ?? [];
  }

  /** What Repository `name` has on this machine, once the core has measured it. */
  onThisMachine(name: string): OnThisMachine | undefined {
    return this.#machine?.find((repository) => repository.repository === name);
  }

  /** Asks the core what each Repository has on this machine, which it measures off the window's thread, once
   * the dialog has read the files. An answer to an older ask gives way to a newer one's. */
  async measure() {
    const asked = ++this.#measures;
    await this.#reading;
    const answer = await ask(commands.onThisMachine());
    if (asked !== this.#measures) return;
    if (answer.status === "error") {
      this.machineProblem = `Tsugiki can't measure the mirrors: ${answer.error.message}`;
      return;
    }
    this.machineProblem = undefined;
    this.#machine = answer.data;
  }

  /** Lists the Repository the add dialog added, which the core has saved and opened, as `home` says, so
   * Cancel keeps it, and measures again. */
  added(home: Home) {
    this.#added = true;
    if (home.screen === "repository") {
      const repository = $state.snapshot(home.repository);
      for (const settings of [this.#before?.settings, this.#settings.settings]) {
        const listed = settings?.repositories.some((one) => one.name === repository.name) ?? true;
        if (!listed) settings?.repositories.push(structuredClone(repository));
      }
      if (this.#before !== undefined) this.filters.add(repository);
    }
    void this.measure();
  }

  /** Reads both files again, so a hand edit made since start-up shows and takes effect (ADR 0014), and says
   * whether it did. If the core can't, the dialog says why and its controls stay off. */
  async open(): Promise<boolean> {
    const reading = ask(commands.rereadSettings());
    this.#reading = reading;
    const answer = await reading;
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
    if (this.applying || this.unsaveable !== undefined) return false;
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
    this.filters.take(loaded.settings.repositories);
    this.#settings.take(loaded);
  }

  /** Lets loads replace the settings again, and puts the theme in force back on the window, unless Apply
   * made the dialog's the theme in force. Once Add repository… has added a Repository, it loads the
   * settings: the core read the files again to add it, so a problem the user has fixed since goes from the
   * status bar, which the loads the dialog held back would have done. */
  #close(): true {
    this.#settings.preview(false);
    void this.#theme.endPreview();
    if (this.#added) void this.#settings.load();
    return true;
  }
}
