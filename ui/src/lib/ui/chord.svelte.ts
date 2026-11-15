import { on } from "svelte/events";
import { type Action, actionOn } from "$lib/state/keymap.svelte";
import { chordName, isChordStart, isModifier, keyName, type Os } from "./keys";

/**
 * The event a chord's action sends to the focused element before the page runs the action. A part of the
 * page that runs the action its own way takes it with `preventDefault`, and the page then runs nothing: the
 * Definition peek runs Go to Definition on its shown hit, as it does on the action's key of one press.
 */
export class ChordAction extends Event {
  static readonly type = "chordaction";
  readonly action: Action;

  constructor(action: Action) {
    super(ChordAction.type, { bubbles: true, cancelable: true });
    this.action = action;
  }
}

/** For `{@attach}`: hands `take` each chord's action that reaches the element, which takes the action
 * when `take` says it ran it. */
export function takesChords(take: (action: Action) => boolean): (node: HTMLElement) => () => void {
  return (node) =>
    on(node, ChordAction.type, (e) => {
      if (e instanceof ChordAction && take(e.action)) e.preventDefault();
    });
}

/** What a chord's action runs, handed the action and the press that ended the chord. */
export type ChordRun = (action: Action, e: KeyboardEvent) => void;

/**
 * The wait for a chord's second key. A chord's first press, such as `Ctrl+K`, starts it, and the next key
 * ends it: a bound one runs its chord's action, and any other key, Esc included, does nothing else. A
 * modifier pressed alone and a held first press leave it waiting. The page that owns it reads every key with
 * it before anything else does, and ends it at a click or when the window loses the focus.
 */
export class ChordWait {
  readonly #os: Os;
  /** The chord's first press, and what its action runs, as the page said where the chord started. */
  #wait = $state.raw<{ readonly first: string; readonly run: ChordRun } | null>(null);

  constructor(os: Os) {
    this.#os = os;
  }

  /** The first key of the chord whose second key Tsugiki waits for, or null while it waits for none. */
  get first(): string | null {
    return this.#wait?.first ?? null;
  }

  /**
   * Reads key press `e`, which the wait then keeps from the browser and from every handler past the
   * window's capture phase, and says whether it did. The performance HUD, which listens in that phase too,
   * still times it. At a chord's first press `start` says what the chord's action runs from where it
   * started, or null where no chord starts.
   */
  read(e: KeyboardEvent, start: (first: string, e: KeyboardEvent) => ChordRun | null): boolean {
    const name = keyName(e, this.#os);
    if (this.#wait === null) {
      const run = name === null || !isChordStart(name) ? null : start(name, e);
      if (name === null || run === null) return false;
      this.#wait = { first: name, run };
    } else if (!e.repeat && !(name === null && isModifier(e))) {
      const { first, run } = this.#wait;
      const action = actionOn(name === null ? null : chordName(first, name));
      this.#wait = null;
      const untaken = action !== undefined && (e.target?.dispatchEvent(new ChordAction(action)) ?? true);
      if (untaken) run(action, e);
    }
    e.preventDefault();
    e.stopPropagation();
    return true;
  }

  /** Ends the wait without running anything. */
  end(): void {
    this.#wait = null;
  }
}
