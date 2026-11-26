// What the last action on an item did, which a floating panel in the window's corner says (#1072).
import type { ItemKey } from "$lib/data/bindings";

/** How long the panel says what an action did, in milliseconds, until the pointer on it holds it. */
export const NOTICE_MS = 5000;

/** What an action did to an item, in the past tense, as the panel says it. */
export type Did =
  "archived" | "unarchived" | "muted" | "unmuted" | "marked seen" | "marked unseen" | "pinned" | "unpinned";

/** What the panel calls the item of `key`: "Issue #141148" or "PR #162138". */
export function itemName(key: ItemKey): string {
  return `${key.kind === "issue" ? "Issue" : "PR"} #${String(key.number)}`;
}

/** What `did` did to the item of `key`, in the past tense: "Issue #141148 archived". */
export function didWords(key: ItemKey, did: Did): string {
  return `${itemName(key)} ${did}`;
}

/** What the user did to the author `login`: "Muted some-author" or "Unmuted some-author". */
export function authorWords(login: string, did: "muted" | "unmuted"): string {
  return `${did === "muted" ? "Muted" : "Unmuted"} ${login}`;
}

/** The room the panel leaves between itself and the bar or the window's edge, in pixels. */
export const PANEL_GAP = 8;

/**
 * The action panel's text and its countdown (#1072). It says what the last action did and goes after five
 * seconds; the pointer on the panel holds the countdown, and leaving resumes it with the time it had left. A
 * second action replaces the words and starts the countdown again, so the panel never stacks.
 */
export class ActionNotice {
  /** What the panel says, or nothing while it says nothing. */
  text = $state<string>();
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** When the countdown ends, while it runs. */
  #endsAt = 0;
  /** How long the countdown has left, while the pointer holds it. */
  #left = 0;
  #held = false;

  /** Says what `words` did, replacing what the panel said, and starts the countdown again. */
  show(words: string) {
    this.text = words;
    this.#left = NOTICE_MS;
    this.#stop();
    // The pointer already on the panel holds the new words too, until it leaves.
    if (!this.#held) this.#arm();
  }

  /** The pointer is on the panel: the countdown waits where it stands. */
  hold() {
    if (this.text === undefined) return;
    this.#held = true;
    this.#left = Math.max(this.#endsAt - Date.now(), 0);
    this.#stop();
  }

  /** The pointer has left the panel: the countdown runs again with the time it had left. */
  release() {
    if (!this.#held) return;
    this.#held = false;
    this.#arm();
  }

  /** Takes the panel away, as when the Repository changes. */
  clear() {
    this.text = undefined;
    this.#held = false;
    this.#stop();
  }

  #arm() {
    this.#endsAt = Date.now() + this.#left;
    this.#timer = setTimeout(() => {
      this.text = undefined;
      this.#timer = undefined;
    }, this.#left);
  }

  #stop() {
    if (this.#timer === undefined) return;
    clearTimeout(this.#timer);
    this.#timer = undefined;
  }
}
