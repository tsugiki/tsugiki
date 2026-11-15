// The tabs of the PR view's rail (#727), and which of them shows. Files is the first; #413 adds Review and
// #686 Search. Each tab names what it is called and the action whose key, palette command and button open
// the rail on it. PrDetail gives each what it draws, and a count where it has one.
import type { Snippet } from "svelte";
import type { Action } from "./keymap.svelte";

/** A tab of the rail: what it is called, and the action that opens the rail on it. */
export type RailTabSpec = { readonly id: string; readonly name: string; readonly action: Action };

/** The rail's tabs, in the order the tab row and the buttons beside the PR's tabs show them. */
export const RAIL_TABS = [
  { id: "files", name: "Files", action: "toggle_rail_files" },
] as const satisfies readonly RailTabSpec[];

export type RailTabId = (typeof RAIL_TABS)[number]["id"];

/** A tab as the rail draws it. */
export type RailTab<T extends string> = {
  readonly id: T;
  readonly name: string;
  /** The number in the label, as in #413's "Review · 3", if the tab counts anything. */
  readonly count?: number;
  /** The key that opens the rail on the tab, and closes it there, or null while `[keys]` leaves it none. */
  readonly key: string | null;
  /** The pane the tab draws. */
  readonly pane: Snippet;
};

/** The tab whose key, palette command and button run `action`, if any does. */
export function railTabOf(action: Action): (typeof RAIL_TABS)[number] | undefined {
  return RAIL_TABS.find((tab) => tab.action === action);
}

/** A tab's label, in the rail's tab row and on its button: its name, and its count where it has one, as
 * #413's "Review · 3". */
export function railTabLabel(name: string, count?: number): string {
  return count === undefined ? name : `${name} · ${String(count)}`;
}

/** Whether the rail shows, and on which of the tabs `T`. It opens on the tab the user picked last, for the
 * session; nothing saves it. */
export class RailState<T extends string> {
  /** Whether the rail shows beside the PR's tabs. It stays open from tab to tab and PR to PR. */
  open = $state(false);
  /** The tab the rail shows, or opens on: the one the user picked last. */
  tab = $state() as T;

  constructor(first: T) {
    this.tab = first;
  }

  /** A tab's button or key, its palette command, or a click on it in the tab row: closes the rail while it
   * shows `tab`, and opens the rail on `tab` otherwise. */
  toggle(tab: T) {
    if (this.open && this.tab === tab) this.open = false;
    else this.pick(tab);
  }

  /** Opens the rail on `tab`, as ← → in the tab row do. */
  pick(tab: T) {
    this.open = true;
    this.tab = tab;
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
