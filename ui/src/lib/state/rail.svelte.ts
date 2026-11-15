// The tabs of the PR view's rail (#727), and which of them shows: Files, then Search (#686); #413 adds
// Review between them. Each tab names what it is called and the action whose key, palette command and
// button open the rail on it. PrDetail gives each what it draws, and a count where it has one.
import type { Snippet } from "svelte";
import type { Action } from "./keymap.svelte";

/** A tab of the rail: what it is called, and the action that opens the rail on it. */
export type RailTabSpec = { readonly name: string; readonly action: Action };

/** The rail's tabs, by what the rest of the window calls them, in the order the tab row and the buttons
 * beside the PR's tabs show them. */
export const RAIL_TABS = {
  files: { name: "Files", action: "toggle_rail_files" },
  search: { name: "Search", action: "search_in_rail" },
} as const satisfies Readonly<Record<string, RailTabSpec>>;

export type RailTabId = keyof typeof RAIL_TABS;

/** The rail's tabs in order. An object keeps its keys in the order they were written, as long as none is a
 * number, and RAIL_TABS has at least one. */
export const RAIL_ORDER = Object.keys(RAIL_TABS) as [RailTabId, ...RailTabId[]];

/** The tab whose key, palette command and button run each action that opens one. */
export const RAIL_TAB_OF: Readonly<Partial<Record<Action, RailTabId>>> = Object.fromEntries(
  RAIL_ORDER.map((id) => [RAIL_TABS[id].action, id]),
);

/** A tab as the rail draws it. */
export type RailTab = RailTabSpec & {
  /** The number in the label, as in #413's "Review · 3", if the tab counts anything. */
  readonly count?: number;
  /** The pane the tab draws. */
  readonly pane: Snippet;
};

/** A tab's label, in the rail's tab row and on its button: its name, and its count where it has one, as
 * #413's "Review · 3". */
export function railTabLabel({ name, count }: RailTab): string {
  return count === undefined ? name : `${name} · ${String(count)}`;
}

/** The tab that Home, End, ← or → in the tab row moves to. */
export type RailStep = "first" | "previous" | "next" | "last";

/** Whether the rail shows, and which of the tabs `T` it shows, in `order`. It opens on the tab the user
 * picked last, for the session; nothing saves it. */
export class RailState<T extends string> {
  /** The tabs in the order the tab row shows them, which the arrows move along. */
  readonly order: readonly T[];
  #open = $state(false);
  #tab = $state() as T;

  constructor(first: T, ...rest: T[]) {
    this.order = [first, ...rest];
    this.#tab = first;
  }

  /** Whether the rail shows beside the PR's tabs. It stays open from tab to tab and PR to PR. */
  get open(): boolean {
    return this.#open;
  }

  /** The tab the rail shows, or opens on: the one the user picked last. */
  get tab(): T {
    return this.#tab;
  }

  /** A tab's button or key, or its palette command: closes the rail while it shows `tab`, and opens the
   * rail on `tab` otherwise. */
  toggle(tab: T) {
    if (this.#open && this.#tab === tab) this.#open = false;
    else this.pick(tab);
  }

  /** Opens the rail on `tab`. */
  pick(tab: T) {
    this.#open = true;
    this.#tab = tab;
  }

  /** Home, End, ← and → in the tab row: shows the first tab, the one before the tab on show, the one after
   * it, or the last, round from either end. */
  go(step: RailStep) {
    const count = this.order.length;
    const at = this.order.indexOf(this.#tab);
    const index: Record<RailStep, number> = {
      first: 0,
      previous: (at + count - 1) % count,
      next: (at + 1) % count,
      last: count - 1,
    };
    const to = index[step];
    // As far as TypeScript knows, `this.order[to]` may be past the end, and a check for that would leave a
    // branch no test reaches, which the coverage gate fails. So this looks for the tab at `to` among them.
    this.order.forEach((tab, index) => {
      if (index === to) this.pick(tab);
    });
  }

  /** Opens the rail on the tab picked last, as #413's Pending review will. */
  reopen() {
    this.#open = true;
  }

  /** × in the rail's corner. */
  close() {
    this.#open = false;
  }
}
