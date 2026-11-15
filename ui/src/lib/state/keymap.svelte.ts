// The keys in force: the core's key table (#548), with the keys `[keys]` in settings.toml gives in place of
// the defaults (#549). Screens bind actions, and find a key's action here. The core reads `[keys]`, and the
// settings hand its remaps over each time they are read.
import { KEYMAP, SHIFTED } from "$lib/data/keymap";

type Default = (typeof KEYMAP)[number];

/** An action the user can bind a key to, named as `[keys]` in settings.toml names it. */
export type Action = Default["action"];

/** A row of the table: an action, its title and where its key works, with its key in force, null for none.
 * A key is named as `keyName` and `chordName` name them: "ctrl+shift+p", "ctrl+k m". */
export type Row = Omit<Default, "key"> & { readonly key: string | null };

/** A row whose action has a key in force. */
export type KeyedRow = Row & { readonly key: string };

/** Where an action's key works, as the core's table says: anywhere, or in the Issue list, the diff or the
 * composer. */
export type Context = Default["context"];

/** The keys `[keys]` gives the actions it names in place of their defaults, as the core read them: a key, or
 * `""` for none. */
export type Remaps = Partial<Record<Action, string>>;

/**
 * The keys the window's guard leaves alone, whichever action has them. The bare ← and → that fold the Issue
 * list by default: the guard would take them from every text box, diff and pane edge, and the list acts on
 * them only while it has the focus. And Ctrl+Shift+I, on which WebView2 opens DevTools in a development build
 * (ADR 0013), which the core lets no remap use.
 */
const LEFT_ALONE: ReadonlySet<string> = new Set(["arrowleft", "arrowright", "ctrl+shift+i"]);

/** Whether `key`, named as `keyName` or `chordName` names it, is a chord: "ctrl+k z". */
export function isChord(key: string): boolean {
  return key.includes(" ");
}

/** The first press of `chord`, named as `keyName` names it: "ctrl+k" for "ctrl+k z". */
export function chordFirst(chord: string): string {
  return chord.slice(0, chord.indexOf(" "));
}

class Keymap {
  #remaps = $state.raw<Remaps>({});

  /** Every row, in the order the Keyboard page lists them. A remapped action keeps its key over an action
   * that has it by default, which is left with no key. */
  readonly rows: readonly Row[] = $derived.by(() => {
    const taken = new Set<string | null>(Object.values(this.#remaps));
    return KEYMAP.map((row) => {
      const given = this.#remaps[row.action];
      if (given !== undefined) return { ...row, key: given === "" ? null : given };
      return taken.has(row.key) ? { ...row, key: null } : row;
    });
  });

  /** The action on each key. Of two remapped actions on one key, the later in the table keeps it, until #550
   * settles that as `[keys]` orders them. */
  readonly #actionOn = $derived(
    new Map<string | null, Action>(
      this.rows.flatMap((row) => (row.key === null ? [] : [[row.key, row.action]])),
    ),
  );

  readonly #keyOf = $derived(
    new Map<Action, string>(this.rows.flatMap((row) => (row.key === null ? [] : [[row.action, row.key]]))),
  );

  /** The first press of each chord. */
  readonly #starts: ReadonlySet<string> = $derived(
    new Set(this.rows.flatMap((row) => (row.key !== null && isChord(row.key) ? [chordFirst(row.key)] : []))),
  );

  /** The keys of one press that the guard owns, and the chords' first presses. Most have no action until
   * their stage arrives. */
  readonly #owned: ReadonlySet<string> = $derived(
    new Set([
      ...this.rows.flatMap((row) =>
        row.key === null || isChord(row.key) || LEFT_ALONE.has(row.key) ? [] : [row.key],
      ),
      ...this.#starts,
    ]),
  );

  /** Puts `remaps` in force in place of those before. */
  remap(remaps: Remaps) {
    this.#remaps = remaps;
  }

  actionOn(name: string | null): Action | undefined {
    return this.#actionOn.get(name);
  }

  keyOf(action: Action): string | null {
    return this.#keyOf.get(action) ?? null;
  }

  isChordStart(name: string): boolean {
    return this.#starts.has(name);
  }

  owns(name: string): boolean {
    return this.#owned.has(name);
  }
}

const keymap = new Keymap();

/** Every row of the table, with the keys in force, in the order the Keyboard page lists them. */
export function keyTable(): readonly Row[] {
  return keymap.rows;
}

/** The rows of the actions whose key works in `context` and runs them, in the table's order: of two remapped
 * actions on one key, only the one `actionOn` finds there, and not a key of one press that also starts a
 * chord, whose wait takes the press. The `?` list shows them. */
export function keysIn(context: Context): readonly KeyedRow[] {
  return keymap.rows.filter(
    (row): row is KeyedRow =>
      row.context === context &&
      row.key !== null &&
      keymap.actionOn(row.key) === row.action &&
      (isChord(row.key) || !keymap.isChordStart(row.key)),
  );
}

/** Puts the keys `[keys]` gives in force, in place of those it gave before. */
export function remapKeys(remaps: Remaps): void {
  keymap.remap(remaps);
}

/** The action on `name`, a key or chord as `keyName` or `chordName` names it, if any. It asks no context: the
 * default table puts each key on one action, which a core test checks, and a remap keeps its key over a
 * default. #550 settles the clashes a remap can still make, such as a key that starts another's chord. */
export function actionOn(name: string | null): Action | undefined {
  return keymap.actionOn(name);
}

/** The key of `action`, or null while it has none. */
export function keyOf(action: Action): string | null {
  return keymap.keyOf(action);
}

/** Whether `name`, a press as `keyName` names it, starts a chord. */
export function isChordStart(name: string): boolean {
  return keymap.isChordStart(name);
}

/** What Shift makes `key` type on a US keyboard, by the core's table: "?" for "/". The window names a key it
 * placed by where it is with Shift by this, as the core suggests in a problem with `[keys]`. */
export function shiftedOnUsKeyboard(key: string): string | undefined {
  return SHIFTED[key];
}

/** Whether `first`, a press as `keyName` names it, starts the chord of one of `actions`. */
export function startsChordOf(first: string, actions: Iterable<Action>): boolean {
  return [...actions].some((action) => {
    const key = keyOf(action);
    return key !== null && isChord(key) && chordFirst(key) === first;
  });
}

/** Whether the window's guard owns `name`, a press as `keyName` names it, and stops the browser for it. */
export function ownsKey(name: string): boolean {
  return keymap.owns(name);
}
