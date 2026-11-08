// How wide the user dragged the Sidebar and the Workbench list (#187). The core keeps the widths in
// `machine.toml`, so they stay on this machine and aren't synced.
import { commands } from "$lib/data/bindings";

/** The Sidebar's width until the user drags its edge, in CSS pixels. */
export const SIDEBAR_WIDTH = 200;
/** The Sidebar's least and greatest widths. */
export const SIDEBAR_LEAST = 140;
export const SIDEBAR_MOST = 480;
/** The Workbench list's least width. It gives way to the Detail pane in a narrow window. */
export const LIST_LEAST = 280;
/** The Detail pane's least width. */
export const DETAIL_LEAST = 320;

export type Pane = "sidebar" | "list";

export class PanesState {
  /** Each pane's width, or nothing for its default: 200 px for the Sidebar, and half the room beside the
   * Detail pane for the list. */
  sidebar = $state<number | null>(null);
  list = $state<number | null>(null);
  #readProblem = $state<string>();
  #saveProblem = $state<string>();
  /** The last save, which the next one waits for, so the widths reach the file in the order they changed. */
  #saving: Promise<void> = Promise.resolve();

  /** Reads the widths the core saved, each kept within its pane's limits, as someone may have written
   * `machine.toml` by hand. Until the core answers, the panes take their defaults, and a pane the user
   * drags meanwhile keeps the width they gave it. */
  async load() {
    try {
      const { sidebar, list } = await commands.paneWidths();
      this.sidebar ??= sidebar === null ? null : Math.min(SIDEBAR_MOST, Math.max(SIDEBAR_LEAST, sidebar));
      this.list ??= list === null ? null : Math.max(LIST_LEAST, list);
    } catch (e) {
      this.#readProblem = `Tsugiki can't read the panes' widths: ${String(e)}`;
    }
  }

  /** Why the widths couldn't be saved, or else read. */
  get problem(): string | undefined {
    return this.#saveProblem ?? this.#readProblem;
  }

  /** Gives `pane` its default width again, and saves that. */
  reset(pane: Pane): Promise<void> {
    this[pane] = null;
    return this.save();
  }

  /** Saves both widths in `machine.toml`, once the save before has finished. */
  save(): Promise<void> {
    this.#saving = this.#saving.then(() => this.#send());
    return this.#saving;
  }

  async #send() {
    const answer = await commands
      .savePaneWidths({ sidebar: this.sidebar, list: this.list })
      .catch((e: unknown) => ({ status: "error" as const, error: { message: String(e) } }));
    this.#saveProblem =
      answer.status === "error" ? `Tsugiki can't save the panes' widths: ${answer.error.message}` : undefined;
  }
}
