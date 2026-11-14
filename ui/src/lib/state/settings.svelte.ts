// The settings in force. The core owns them and reads the files when it starts and when the Settings dialog
// opens, so this asks at start and holds what the dialog hands it (ADR 0014).
import {
  commands,
  type DiffFiles,
  type DiffPeek,
  type LanguageServerSettings_Serialize,
  type LoadedSettings_Serialize,
  type Settings_Serialize,
} from "$lib/data/bindings";

/** What `settings.toml` holds, as the core read it. */
export type Settings = Settings_Serialize;
/** Where each language server is, and the idle time, as `machine.toml` holds them (#683). */
export type LanguageServerSettings = LanguageServerSettings_Serialize;
/** The settings, with what was wrong with the files the core read them from. */
export type LoadedSettings = LoadedSettings_Serialize;

export class SettingsState {
  /** The settings the core read, or those the Settings dialog previews while it's open. Unset until the core
   * answers. */
  settings = $state<Settings>();
  /** The language servers' settings the core read, or those the Settings dialog edits while it's open. */
  languageServers = $state<LanguageServerSettings>();
  problems = $state<string[]>([]);
  /** Whether the core couldn't be asked at all. */
  #unasked = $state(false);
  // Off until the core answers, so a status bar the settings hide doesn't appear for a moment.
  statusBarVisible = $derived(this.settings?.status_bar.visible ?? this.#unasked);
  /** Whether each PR's Files tab opens on all its files or one at a time (#605). */
  diffFiles: DiffFiles = $derived(this.settings?.diff.files ?? "all");
  /** The place each Definition peek opens in: under its line, or split below the code (#459). */
  diffPeek: DiffPeek = $derived(this.settings?.diff.peek ?? "inline");
  /** How many times settings were taken, so a load answered after a newer take leaves them be. */
  #takes = 0;
  /** Whether the Settings dialog previews changes on these settings, which a load mustn't replace. */
  #previewing = false;

  /** Asks the core for the settings it last read. Its answer gives way to settings taken since it asked, and
   * to the Settings dialog's changes while they show. */
  async load() {
    const takes = this.#takes;
    try {
      const loaded = await commands.settings();
      if (takes === this.#takes && !this.#previewing) this.take(loaded);
    } catch (e) {
      this.#unasked = true;
      this.problems = [`Tsugiki can't read its settings: ${String(e)}`];
    }
  }

  /** Holds what the core read, and what was wrong with the files it read them from. */
  take({ settings, language_servers, problems }: LoadedSettings) {
    this.#takes += 1;
    this.settings = settings;
    this.languageServers = language_servers;
    this.problems = problems;
  }

  /** Keeps a load from replacing the settings while the Settings dialog previews on them, or lets it
   * again. */
  preview(on: boolean) {
    this.#previewing = on;
  }
}
