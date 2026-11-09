// How the user laid out the panes: how wide they dragged the Sidebar and the Workbench list (#187), and
// whether the Sidebar shows (#234). The core keeps the layout in `machine.toml`, so it stays on this machine
// and isn't synced.
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
  /** Whether `Ctrl+B` hid the Sidebar, or nothing until the core or the user says. */
  #sidebarHidden = $state<boolean | null>(null);
  #readProblem = $state<string>();
  #saveProblem = $state<string>();
  /** The last save, or else the first read, which the next save waits for. The layout then reaches the file
   * in the order it changed, and a save never sends a default the file holds something else for. */
  #saving: Promise<void> = Promise.resolve();

  /** Reads the layout the core saved, each width kept within its pane's limits, as someone may have written
   * `machine.toml` by hand. Until the core answers, the panes take their defaults, and what the user changes
   * meanwhile stays as they left it. */
  load(): Promise<void> {
    const reading = this.#read();
    this.#saving = this.#saving.then(() => reading);
    return reading;
  }

  async #read() {
    try {
      const { sidebar, list, sidebar_hidden } = await commands.paneLayout();
      this.sidebar ??= sidebar === null ? null : Math.min(SIDEBAR_MOST, Math.max(SIDEBAR_LEAST, sidebar));
      this.list ??= list === null ? null : Math.max(LIST_LEAST, list);
      this.#sidebarHidden ??= sidebar_hidden;
    } catch (e) {
      this.#readProblem = `Tsugiki can't read the panes' layout: ${String(e)}`;
    }
  }

  /** Whether `Ctrl+B` hid the Sidebar. It keeps its width for when it shows again. */
  get sidebarHidden(): boolean {
    return this.#sidebarHidden ?? false;
  }

  /** Why the layout couldn't be saved, or else read. */
  get problem(): string | undefined {
    return this.#saveProblem ?? this.#readProblem;
  }

  /** Hides the Sidebar, or shows it again, and saves that. */
  toggleSidebar(): Promise<void> {
    this.#sidebarHidden = !this.sidebarHidden;
    return this.save();
  }

  /** Gives `pane` its default width again, and saves that. */
  reset(pane: Pane): Promise<void> {
    this[pane] = null;
    return this.save();
  }

  /** Saves the layout in `machine.toml`, once the save before has finished. */
  save(): Promise<void> {
    this.#saving = this.#saving.then(() => this.#send());
    return this.#saving;
  }

  /** Never rejects, as a rejection would stop every save after it. */
  async #send() {
    const layout = { sidebar: this.sidebar, list: this.list, sidebar_hidden: this.sidebarHidden };
    let problem: string | undefined;
    try {
      const answer = await commands.savePaneLayout(layout);
      problem = answer.status === "error" ? answer.error.message : undefined;
    } catch (e) {
      problem = String(e);
    }
    this.#saveProblem =
      problem === undefined ? undefined : `Tsugiki can't save the panes' layout: ${problem}`;
  }
}
