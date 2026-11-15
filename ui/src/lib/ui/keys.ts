// Tsugiki's keys (ADR 0013). The window's guard runs before anything else sees a key, and stops the browser
// for every key Tsugiki owns, so a key with no action yet can't print or reload instead. Only the wait for a
// chord's second key comes first: the key that ends it goes to neither. The keys come from the core's key
// table, and screens bind actions, not keys (#548).
import { type Action, actionOn, KEY_TABLE, type Key, type KeyedAction, keyOf } from "$lib/state/keymap";

export type Os = "mac" | "other";

/** A chord of the key table, named as `chordName` names it: "ctrl+k z". */
type OwnedChord = Extract<Key, `${string} ${string}`>;

/** The first key of each chord. */
export type ChordStart = OwnedChord extends `${infer First} ${string}` ? First : never;

/** A key of one press, as the table and the chords' first keys have them. */
type OwnedKey = Exclude<Key, OwnedChord> | ChordStart;

/** What each bound action does. The guard runs the action of a key, and the wait for a chord's second key
 * the action of a chord. */
export type Bindings = Readonly<Partial<Record<Action, () => void>>>;

function isChord(key: Key): key is OwnedChord {
  return key.includes(" ");
}

/**
 * The actions whose keys the guard leaves alone. The bare ← and → that fold the Issue list: the guard would
 * take them from every text box, diff and pane edge, and the list acts on them only while it has the focus.
 * And Ctrl+Shift+I, on which WebView2 opens DevTools in a development build (ADR 0013).
 */
const LEFT_ALONE: ReadonlySet<Action> = new Set(["unfold", "fold", "open_devtools"]);

const CHORDS: readonly OwnedChord[] = KEY_TABLE.flatMap((row) =>
  row.key !== null && isChord(row.key) ? [row.key] : [],
);

const STARTS: ReadonlySet<string> = new Set(CHORDS.map((chord) => chord.slice(0, chord.indexOf(" "))));

export function isChordStart(name: string): name is ChordStart {
  return STARTS.has(name);
}

/** The keys of one press that the guard owns, named as `keyName` names them, and the chords' first keys.
 * Most have no action until their stage arrives. */
const OWNED: ReadonlySet<string> = new Set([
  ...KEY_TABLE.flatMap((row) =>
    row.key === null || isChord(row.key) || LEFT_ALONE.has(row.action) ? [] : [row.key],
  ),
  ...STARTS,
]);

/**
 * A chord as a binding: its two presses, each named by `keyName`, joined by a space, such as "ctrl+k z". The
 * Settings Keyboard page shows and remaps a chord by this name.
 */
export function chordName(first: ChordStart, second: string): string {
  return `${first} ${second}`;
}

/** Whether `name` types in a text box: a plain key, a capital letter such as Shift+Q, or the key of Hide or
 * Show the Issue List, which doesn't work in one, as its default Ctrl+Alt+B is a key AltGr sends to type on
 * Windows. */
function typesInTextBox(name: string): boolean {
  return /^(?:shift\+)?.$/.test(name) || actionOn(name) === "toggle_issue_list";
}

const MODIFIERS = new Set(["control", "shift", "alt", "meta", "altgraph", "capslock", "numlock", "fn"]);

/** Whether `e` presses a modifier or a lock alone, which `keyName` has no name for. */
export function isModifier(e: KeyboardEvent): boolean {
  return MODIFIERS.has(e.key.toLowerCase());
}

const PUNCTUATION: Readonly<Record<string, string>> = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  Backslash: "\\",
};

/** The Latin character at a physical key: "KeyP" is "p", "Digit0" is "0", "Comma" is ",". */
function characterAt(code: string): string | undefined {
  const named = /^(?:Key|Digit)(.)$/.exec(code);
  return named?.[1]?.toLowerCase() ?? PUNCTUATION[code];
}

/**
 * Whether `e.key` says nothing about a binding, so the key has to go by where it is. On a non-Latin layout
 * `e.key` is what the key types, so Ctrl+P on a Russian keyboard says "з", and a dead key says "Dead".
 * Elsewhere a key goes by what it types, as in VS Code, and that includes a letter like "ß" on a German
 * keyboard.
 */
function goesByPlace(key: string): boolean {
  return (
    key === "Dead" || key === "Unidentified" || /^(?!\p{Script=Latin})(?=\P{ASCII})[\p{L}\p{N}]$/u.test(key)
  );
}

/**
 * A key press as a binding: "ctrl+shift+p", "f7", "?". On macOS Cmd is called ctrl, as
 * `docs/devel/keyboard.md` has it. Null for a key that can't be a binding: a modifier alone, AltGr, which
 * types a character but reports Ctrl and Alt, and the Windows key or macOS's Ctrl, which belong to the OS
 * and to text boxes. On macOS a key with Option goes by where it is, as Option makes it type something
 * else: Option+B types "∫".
 */
export function keyName(e: KeyboardEvent, os: Os): string | null {
  const [primary, foreign] = os === "mac" ? [e.metaKey, e.ctrlKey] : [e.ctrlKey, e.metaKey];
  const byPlace = goesByPlace(e.key) || (os === "mac" && e.altKey);
  const key = (byPlace ? characterAt(e.code) : undefined) ?? e.key.toLowerCase();
  if (foreign || MODIFIERS.has(key) || e.getModifierState("AltGraph")) {
    return null;
  }
  // A symbol typed with Shift already says so: "?" is Shift+/ on one layout and something else on another. A
  // key named by its place doesn't.
  const shift = e.shiftKey && (byPlace || key.length > 1 || /[a-z]/.test(key));
  const parts = [primary && "ctrl", e.altKey && "alt", shift && "shift", key === " " ? "space" : key];
  return parts.filter(Boolean).join("+");
}

const MAC_NAMES: Readonly<Record<string, string>> = { ctrl: "Cmd", alt: "Option" };
const KEY_NAMES: Readonly<Record<string, string>> = {
  arrowleft: "←",
  arrowright: "→",
  arrowup: "↑",
  arrowdown: "↓",
  pagedown: "PageDown",
  pageup: "PageUp",
};

/** A binding as the user reads it: "ctrl+alt+b" is "Ctrl+Alt+B", or "Cmd+Option+B" on macOS,
 * "alt+arrowleft" is "Alt+←", and "alt+pagedown" is "Alt+PageDown". */
export function keyLabel(name: OwnedKey, os: Os): string {
  return name
    .split("+")
    .map(
      (part) =>
        (os === "mac" ? MAC_NAMES[part] : undefined) ??
        KEY_NAMES[part] ??
        part.charAt(0).toUpperCase() + part.slice(1),
    )
    .join("+");
}

/** A chord as the user reads it: "ctrl+k o" is "Ctrl+K O", or "Cmd+K O" on macOS. */
function chordLabel(chord: OwnedChord, os: Os): string {
  const [first, second] = chord.split(" ") as [ChordStart, string];
  return `${keyLabel(first, os)} ${second.toUpperCase()}`;
}

/** The key of `action` as the user reads it, such as "Ctrl+K O" for All Files or One File at a Time. */
export function actionLabel(action: KeyedAction, os: Os): string {
  const key = keyOf(action);
  return isChord(key) ? chordLabel(key, os) : keyLabel(key, os);
}

/** Which OS this is, for `keyName`. */
export function currentOs(platform: string = navigator.platform): Os {
  return platform.startsWith("Mac") ? "mac" : "other";
}

/** Whether click `e` asks for Go to definition: Ctrl+click, or Cmd+click on macOS, where Ctrl+click is the
 * system's (#444). */
export function goesToDefinition(e: MouseEvent, os: Os): boolean {
  return os === "mac" ? e.metaKey : e.ctrlKey;
}

export function isTextBox(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

/**
 * The handler for the window's capture phase. In a text box a plain key and Ctrl+Alt+B type, and any other
 * key with a modifier is still Tsugiki's. Every build owns F12, which is Go to definition: a development
 * build opens DevTools on WebView2's own Ctrl+Shift+I, which Tsugiki leaves alone (ADR 0013).
 */
export function keyGuard(bindings: Bindings, os: Os): (e: KeyboardEvent) => void {
  return (e) => {
    const name = keyName(e, os);
    if (name === null || !OWNED.has(name) || (typesInTextBox(name) && isTextBox(e.target))) return;
    e.preventDefault();
    const action = actionOn(name);
    if (action !== undefined) bindings[action]?.();
  };
}
