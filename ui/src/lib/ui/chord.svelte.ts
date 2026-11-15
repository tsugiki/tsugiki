import { actionOn } from "$lib/state/keymap.svelte";
import { type Bindings, chordName, isChordStart, isModifier, isTextBox, keyName, type Os } from "./keys";

/**
 * The wait for a chord's second key. `Ctrl+K` starts it, and the next key ends it: a bound one runs its
 * chord's action, and any other key, Esc included, does nothing else. A modifier pressed alone and a held
 * `Ctrl+K` leave it waiting. The page that owns it reads every key with it before anything else does, and
 * ends it at a click or when the window loses the focus.
 */
export class ChordWait {
  readonly #os: Os;
  #first = $state<string | null>(null);

  constructor(os: Os) {
    this.#os = os;
  }

  /** The first key of the chord whose second key Tsugiki waits for, or null while it waits for none. */
  get first(): string | null {
    return this.#first;
  }

  /**
   * Reads key press `e`, which the wait then keeps from the browser and from every handler past the
   * window's capture phase, and says whether it did. The performance HUD, which listens in that phase too,
   * still times it. `canStart` says whether a chord can start here: not under a dialog or the palette, nor
   * on a screen with no chords. A chord never starts in a text box, which keeps its keys.
   */
  read(e: KeyboardEvent, canStart: boolean, bindings: Bindings): boolean {
    const name = keyName(e, this.#os);
    if (this.#first === null) {
      if (name === null || !isChordStart(name) || !canStart || isTextBox(e.target)) return false;
      this.#first = name;
    } else if (!e.repeat && !(name === null && isModifier(e))) {
      const action = actionOn(name === null ? null : chordName(this.#first, name));
      this.#first = null;
      if (action !== undefined) bindings[action]?.(e);
    }
    e.preventDefault();
    e.stopPropagation();
    return true;
  }

  /** Ends the wait without running anything. */
  end(): void {
    this.#first = null;
  }
}
