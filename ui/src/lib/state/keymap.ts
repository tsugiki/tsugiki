// The core's key table (#548): each action the user can bind, its default key and where it works. Screens
// bind actions, and find a key's action here.
import { KEYMAP } from "$lib/data/keymap";

type Row = (typeof KEYMAP)[number];

/** An action the user can bind a key to, named as `[keys]` in settings.toml names it. */
export type Action = Row["action"];

/** A key or chord of the table, named as `keyName` and `chordName` name them: "ctrl+shift+p", "ctrl+k m". */
export type Key = NonNullable<Row["key"]>;

/** An action that has a key. Reset Layout, say, has none until the user gives it one. */
export type KeyedAction = Extract<Row, { key: string }>["action"];

/** Every row of the table, in the order the Keyboard page lists them. */
export const KEY_TABLE: readonly Row[] = KEYMAP;

const ACTION_ON = new Map<string | null, Action>(
  KEYMAP.flatMap((row) => (row.key === null ? [] : [[row.key, row.action] as const])),
);

// `Object.fromEntries` forgets which action each key belongs to, though every keyed action has a row.
const KEY_OF = Object.fromEntries(
  KEYMAP.flatMap((row) => (row.key === null ? [] : [[row.action, row.key] as const])),
) as Record<KeyedAction, Key>;

/** The action on `name`, a key or chord as `keyName` or `chordName` names it, if any. It asks no context, as
 * the table puts each key on one action, which a core test checks. */
export function actionOn(name: string | null): Action | undefined {
  return ACTION_ON.get(name);
}

/** The key of `action`. */
export function keyOf(action: KeyedAction): Key {
  return KEY_OF[action];
}
