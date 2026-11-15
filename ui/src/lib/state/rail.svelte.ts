// The tabs of the PR view's rail (#727), and which of them shows. Files is the first; #413 adds Review and
// #686 Search. Each tab names what it is called and the action whose key, palette command and button open
// the rail on it. PrDetail gives each what it draws, and a count where it has one.
import type { Snippet } from "svelte";
import type { Action } from "./keymap.svelte";

/** A tab of the rail: what it is called, and the action that opens the rail on it. */
export type RailTabSpec = { readonly name: string; readonly action: Action };

/** The rail's tabs, by what the rest of the window calls them. */
export const RAIL_TABS = {
  files: { name: "Files", action: "toggle_rail_files" },
} as const satisfies Readonly<Record<string, RailTabSpec>>;

export type RailTabId = keyof typeof RAIL_TABS;

/** The rail's tabs in the order the tab row and the buttons beside the PR's tabs show them, each once. */
export const RAIL_ORDER = ["files"] as const satisfies readonly RailTabId[];

/** A tab as the rail draws it. */
export type RailTab = RailTabSpec & {
  /** The number in the label, as in #413's "Review · 3", if the tab counts anything. */
  readonly count?: number;
  /** The pane the tab draws. */
  readonly pane: Snippet;
};

/** The tab whose key, palette command and button run `action`, if any does. */
export function railTabOf(action: Action): RailTabId | undefined {
  return RAIL_ORDER.find((tab) => RAIL_TABS[tab].action === action);
}

/** A tab's label, in the rail's tab row and on its button: its name, and its count where it has one, as
 * #413's "Review · 3". */
export function railTabLabel({ name, count }: RailTab): string {
  return count === undefined ? name : `${name} · ${String(count)}`;
}

/** Whether the rail shows, and which of the tabs `T` it shows, in `order`. It opens on the tab the user
 * picked last, for the session; nothing saves it. */
export class RailState<T extends string> {
  /** The tabs in the order the tab row shows them, which ← → move along. */
  readonly order: readonly T[];
  /** Whether the rail shows beside the PR's tabs. It stays open from tab to tab and PR to PR. */
  open = $state(false);
  /** The tab the rail shows, or opens on: the one the user picked last. */
  tab = $state() as T;

  constructor(first: T, ...rest: T[]) {
    this.order = [first, ...rest];
    this.tab = first;
  }

  /** A tab's button or key, its palette command, or a click on it in the tab row: closes the rail while it
   * shows `tab`, and opens the rail on `tab` otherwise. */
  toggle(tab: T) {
    if (this.open && this.tab === tab) this.open = false;
    else this.pick(tab);
  }

  /** Opens the rail on `tab`. */
  pick(tab: T) {
    this.open = true;
    this.tab = tab;
  }

  /** ← and → in the tab row: shows the tab after the one on show, or before it, round from either end. */
  step(by: 1 | -1) {
    const to = (this.order.indexOf(this.tab) + by + this.order.length) % this.order.length;
    // As far as TypeScript knows, `this.order[to]` may be past the end, and a check for that would leave a
    // branch no test reaches, which the coverage gate fails. So this looks for the tab at `to` among them.
    this.order.forEach((tab, at) => {
      if (at === to) this.pick(tab);
    });
  }

  /** Opens the rail on the tab picked last, as #413's Pending review will. */
  reopen() {
    this.open = true;
  }

  /** × in the rail's corner. */
  close() {
    this.open = false;
  }
}
