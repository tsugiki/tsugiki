// What the Settings dialog (docs/devel/settings.md) does with the settings: it reads the files again as it
// opens, previews each change on the window at once, and Apply saves them or Cancel puts them back (#520).
import { commands, type SettingsChanges_Deserialize } from "$lib/data/bindings";
import { ask } from "./answer";
import type { Settings, SettingsState } from "./settings.svelte";

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

export class SettingsDialog {
  readonly #settings: SettingsState;
  shown = $state<Shown>({ page: "General" });
  /** The settings once the dialog read the files, which Cancel puts back. Unset until then, and for good if
   * it couldn't read them. */
  #before = $state.raw<Settings>();
  /** Why the dialog couldn't read the files again or save to them, until the next try. */
  problem = $state<string>();
  /** Whether Apply's save is on its way, which Cancel can't take back. */
  applying = $state(false);

  /** The dialog previews on `settings`, so a load mustn't replace them until it closes. */
  constructor(settings: SettingsState) {
    this.#settings = settings;
    settings.preview(true);
  }

  /** The settings the dialog shows and changes, once it has read the files. */
  get current(): Settings | undefined {
    return this.#before === undefined ? undefined : this.#settings.settings;
  }

  /** Each table the dialog edits that differs from what the files said as it opened, which Apply saves. */
  get changes(): SettingsChanges_Deserialize {
    const { current } = this;
    const before = this.#before;
    if (current === undefined || before === undefined) return {};
    const differs = (table: Edited) => JSON.stringify(current[table]) !== JSON.stringify(before[table]);
    return {
      ...(differs("status_bar") && { status_bar: current.status_bar }),
      ...(differs("releases") && { releases: current.releases }),
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
    this.#settings.take(answer.data);
    this.#before = $state.snapshot(this.#settings.settings);
    return true;
  }

  /** Changes the settings in force, which the window shows at once. */
  change(edit: (settings: Settings) => void) {
    if (this.current !== undefined) edit(this.current);
  }

  /** Puts back what the files said as the dialog opened, and says whether it may close: not while Apply
   * saves. */
  cancel(): boolean {
    if (this.applying) return false;
    if (this.#before !== undefined) this.#settings.settings = structuredClone(this.#before);
    return this.#close();
  }

  /** Saves what changed, and says whether the dialog may close: with nothing changed it closes at once, as
   * OK, and a save the core refused keeps it open with why. */
  async apply(): Promise<boolean> {
    if (this.applying) return false;
    const changes = $state.snapshot(this.changes);
    if (Object.keys(changes).length === 0) return this.#close();
    this.applying = true;
    const answer = await ask(commands.applySettings(changes));
    this.applying = false;
    if (answer.status === "error") {
      this.problem = `Tsugiki can't save the settings: ${answer.error.message}`;
      return false;
    }
    this.#settings.take(answer.data);
    return this.#close();
  }

  #close(): true {
    this.#settings.preview(false);
    return true;
  }
}
