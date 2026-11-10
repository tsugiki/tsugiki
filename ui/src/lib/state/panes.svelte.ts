// How the user laid out the panes: how wide they dragged the Sidebar and the Workbench list (#187), and
// whether the Sidebar (#234) and the list (#242) show, and which of them `Ctrl+K Z` hid (#246). The core
// keeps the layout in `machine.toml`, so it stays on this machine and isn't synced.
import { commands, type Panes } from "$lib/data/bindings";

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
  /** Whether `Ctrl+B` hid the Sidebar and `Ctrl+Alt+B` the list, each nothing until the core or the user
   * says. */
  #hidden = $state<Record<Pane, boolean | null>>({ sidebar: null, list: null });
  /** The panes `Ctrl+K Z` hid, which it brings back next: null while it remembers none, and undefined until
   * the core or the user says. */
  #detailOnlyHid = $state<Panes | null | undefined>();
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
      const { sidebar, list, sidebar_hidden, list_hidden, detail_only_hid } = await commands.paneLayout();
      this.sidebar ??= sidebar === null ? null : Math.min(SIDEBAR_MOST, Math.max(SIDEBAR_LEAST, sidebar));
      this.list ??= list === null ? null : Math.max(LIST_LEAST, list);
      this.#hidden.sidebar ??= sidebar_hidden;
      this.#hidden.list ??= list_hidden;
      if (this.#detailOnlyHid === undefined) this.#detailOnlyHid = detail_only_hid;
    } catch (e) {
      this.#readProblem = `Tsugiki can't read the panes' layout: ${String(e)}`;
    }
  }

  /** Whether `pane` is hidden: the Sidebar by `Ctrl+B`, the list by `Ctrl+Alt+B`. It keeps its width for
   * when it shows again. */
  hidden(pane: Pane): boolean {
    return this.#hidden[pane] ?? false;
  }

  /** Why the layout couldn't be saved, or else read. */
  get problem(): string | undefined {
    return this.#saveProblem ?? this.#readProblem;
  }

  /** Whether the Detail pane shows alone, with both the Sidebar and the list hidden. */
  get detailOnly(): boolean {
    return this.hidden("sidebar") && this.hidden("list");
  }

  /** Hides `pane`, or shows it again, and saves that. `toggleDetailOnly` then forgets what it hid. */
  toggle(pane: Pane): Promise<void> {
    this.#hidden[pane] = !this.hidden(pane);
    this.#detailOnlyHid = null;
    return this.save();
  }

  /** Shows `pane` if it's hidden, or may be once the core answers, and saves that. `toggleDetailOnly` then
   * forgets what it hid. */
  show(pane: Pane): Promise<void> {
    if (this.#hidden[pane] === false) return Promise.resolve();
    this.#hidden[pane] = false;
    this.#detailOnlyHid = null;
    return this.save();
  }

  /** `Ctrl+K Z`: while a pane shows, hides both and remembers which showed. While neither shows, brings back
   * the ones it remembers, or both when it remembers none. Saves that. */
  toggleDetailOnly(): Promise<void> {
    if (this.detailOnly) {
      const back = this.#detailOnlyHid ?? "both";
      this.#hidden.sidebar = back === "list";
      this.#hidden.list = back === "sidebar";
      this.#detailOnlyHid = null;
    } else {
      this.#detailOnlyHid = this.hidden("sidebar") ? "list" : this.hidden("list") ? "sidebar" : "both";
      this.#hidden.sidebar = true;
      this.#hidden.list = true;
    }
    return this.save();
  }

  /** Gives `pane` its default width again, and saves that. */
  reset(pane: Pane): Promise<void> {
    this[pane] = null;
    return this.save();
  }

  /** "Reset Layout": gives both panes their default widths and shows both, forgets what `Ctrl+K Z` hid, and
   * saves that. */
  resetLayout(): Promise<void> {
    this.sidebar = null;
    this.list = null;
    this.#hidden.sidebar = false;
    this.#hidden.list = false;
    this.#detailOnlyHid = null;
    return this.save();
  }

  /** Saves the layout in `machine.toml`, once the save before has finished. */
  save(): Promise<void> {
    this.#saving = this.#saving.then(() => this.#send());
    return this.#saving;
  }

  /** Never rejects, as a rejection would stop every save after it. */
  async #send() {
    const layout = {
      sidebar: this.sidebar,
      list: this.list,
      sidebar_hidden: this.hidden("sidebar"),
      list_hidden: this.hidden("list"),
      detail_only_hid: this.#detailOnlyHid ?? null,
    };
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
