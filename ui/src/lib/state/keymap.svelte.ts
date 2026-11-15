// The keys in force: the core's key table (#548), with the keys `[keys]` in settings.toml gives in place of
// the defaults (#549). Screens bind actions, and find a key's action here. The core reads `[keys]`, and the
// settings hand its remaps over each time they are read.
import { KEYMAP } from "$lib/data/keymap";

type Default = (typeof KEYMAP)[number];

/** An action the user can bind a key to, named as `[keys]` in settings.toml names it. */
export type Action = Default["action"];

/** A row of the table: an action, its title and where its key works, with its key in force, null for none.
 * A key is named as `keyName` and `chordName` name them: "ctrl+shift+p", "ctrl+k m". */
export type Row = Omit<Default, "key"> & { readonly key: string | null };

/** The keys `[keys]` gives the actions it names in place of their defaults, as the core read them: a key, or
 * `""` for none. */
export type Remaps = Partial<Record<Action, string>>;

/**
 * The actions whose keys the window's guard leaves alone. The bare ← and → that fold the Issue list: the
 * guard would take them from every text box, diff and pane edge, and the list acts on them only while it has
 * the focus. And Ctrl+Shift+I, on which WebView2 opens DevTools in a development build (ADR 0013), which the
 * core refuses to remap.
 */
const LEFT_ALONE: ReadonlySet<Action> = new Set(["unfold", "fold", "open_devtools"]);

/** Whether `key`, named as `keyName` or `chordName` names it, is a chord: "ctrl+k z". */
export function isChord(key: string): boolean {
  return key.includes(" ");
}

class Keymap {
  #remaps = $state.raw<Remaps>({});

  /** Every row, in the order the Keyboard page lists them. */
  readonly rows: readonly Row[] = $derived(
    KEYMAP.map((row) => {
      const given = this.#remaps[row.action];
      return given === undefined ? row : { ...row, key: given === "" ? null : given };
    }),
  );

  /** The action on each key. A remapped action keeps its key over a default one, which comes first so the
   * remap overwrites it. Of two remapped actions on one key, the later in the table keeps it, until #550
   * settles that as `[keys]` orders them. */
  readonly #actionOn = $derived(
    new Map<string | null, Action>(
      [
        ...this.rows.filter((row) => this.#remaps[row.action] === undefined),
        ...this.rows.filter((row) => this.#remaps[row.action] !== undefined),
      ].flatMap((row) => (row.key === null ? [] : [[row.key, row.action]])),
    ),
  );

  readonly #keyOf = $derived(
    new Map<Action, string>(this.rows.flatMap((row) => (row.key === null ? [] : [[row.action, row.key]]))),
  );

  /** The first press of each chord. */
  readonly #starts: ReadonlySet<string> = $derived(
    new Set(
      this.rows.flatMap((row) =>
        row.key !== null && isChord(row.key) ? [row.key.slice(0, row.key.indexOf(" "))] : [],
      ),
    ),
  );

  /** The keys of one press that the guard owns, and the chords' first presses. Most have no action until
   * their stage arrives. */
  readonly #owned: ReadonlySet<string> = $derived(
    new Set([
      ...this.rows.flatMap((row) =>
        row.key === null || isChord(row.key) || LEFT_ALONE.has(row.action) ? [] : [row.key],
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

/** Whether the window's guard owns `name`, a press as `keyName` names it, and stops the browser for it. */
export function ownsKey(name: string): boolean {
  return keymap.owns(name);
}
