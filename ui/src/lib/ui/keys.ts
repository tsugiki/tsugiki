// Tsugiki's keys (ADR 0013). The window's guard runs before anything else sees a key, and stops the browser
// for every key Tsugiki owns, so a key with no action yet can't print or reload instead. Only the wait for a
// chord's second key comes first: the key that ends it goes to neither. The keys come from the core's key
// table, with the user's remaps (#549), and screens bind actions, not keys (#548).
import {
  type Action,
  actionOn,
  isChord,
  keyOf,
  ownsKey,
  shiftedOnUsKeyboard,
} from "$lib/state/keymap.svelte";

export { isChordStart } from "$lib/state/keymap.svelte";

export type Os = "mac" | "other";

/** Where a press went, as the keys, the `?` list and the command palette ask: into the Issue list, the
 * Detail pane or neither, and whether into a text box. It is worked out from the press's target once, as the
 * key comes or the palette opens, so it holds while the element that had the focus is drawn again. */
export interface KeyTarget {
  readonly pane: Pane;
  readonly textBox: boolean;
}

/** A pane a press can go to, or null for neither. */
export type Pane = "list" | "detail" | null;

/** Where a press on `target` went: into the pane `paneOf` finds it in, none unless it's given, and whether
 * into a text box. The Repository's screen knows its panes, and elsewhere a press goes to no pane. */
export function keyTarget(
  target: EventTarget | null,
  paneOf: (target: EventTarget | null) => Pane = () => null,
): KeyTarget {
  return { pane: paneOf(target), textBox: isTextBox(target) };
}

/** What each bound action does, handed the key press that ran it. The guard runs the action of a key, and
 * the wait for a chord's second key the action of a chord. */
export type Bindings = Readonly<Partial<Record<Action, (e: KeyboardEvent) => void>>>;

/**
 * A chord as a binding: its two presses, each named by `keyName`, joined by a space, such as "ctrl+k z". The
 * Settings Keyboard page shows and remaps a chord by this name.
 */
export function chordName(first: string, second: string): string {
  return `${first} ${second}`;
}

/** The actions whose key works in a text box, as long as it types nothing there: the page's own, which the
 * guard runs, and those the Repository screen runs in one. Their key works there though it holds Ctrl and
 * Alt, which AltGr sends on Windows to type: Unified or Side by Side is one, on Ctrl+Alt+U. */
const IN_TEXT_BOX: ReadonlySet<Action> = new Set([
  "command_palette",
  "quick_open",
  "switch_repository",
  "go_home",
  "open_settings",
  "toggle_hud",
  "pick_theme",
  "reload_themes",
  "toggle_sidebar",
  "next_pane",
  "previous_pane",
  "open_on_github",
  "toggle_diff_view",
  "find_in_diff",
  "search_in_rail",
  "show_keys",
]);

/** Whether the key of `action` works in a text box, as long as it types nothing there. */
export function worksInTextBox(action: Action): boolean {
  return IN_TEXT_BOX.has(action);
}

/** Whether `name` types in a text box: a plain key, Backspace and Delete, a capital letter such as Shift+Q,
 * and Ctrl+Alt with a key that types, which AltGr sends on Windows, as on Ctrl+Alt+B or a German Ctrl+Alt+Q,
 * unless its action works in a text box. */
export function typesInTextBox(name: string): boolean {
  if (/^(?:shift\+)?(?:.|backspace|delete)$/.test(name)) return true;
  const action = actionOn(name);
  return /^ctrl\+alt\+(?:shift\+)?.$/.test(name) && (action === undefined || !IN_TEXT_BOX.has(action));
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
 *
 * Shift stays only on a Latin letter and on a key that types nothing, such as the space bar or F7. Any other
 * key goes by what Shift makes it type: "?" is Shift+/ on one layout and something else on another. A key
 * named by its place takes what Shift types there on a US keyboard, so macOS's Option+Shift+/ is "alt+?" as
 * Windows names Alt+Shift+/. The core refuses a line of `[keys]` that keeps Shift elsewhere.
 */
export function keyName(e: KeyboardEvent, os: Os): string | null {
  const [primary, foreign] = os === "mac" ? [e.metaKey, e.ctrlKey] : [e.ctrlKey, e.metaKey];
  const byPlace = goesByPlace(e.key) || (os === "mac" && e.altKey);
  const placed = (byPlace ? characterAt(e.code) : undefined) ?? e.key.toLowerCase();
  if (foreign || MODIFIERS.has(placed) || e.getModifierState("AltGraph")) {
    return null;
  }
  const key = placed === " " ? "space" : placed;
  const shifted = e.shiftKey && byPlace ? shiftedOnUsKeyboard(key) : undefined;
  const shift = e.shiftKey && shifted === undefined && (key.length > 1 || /^[a-z]$/.test(key));
  const parts = [primary && "ctrl", e.altKey && "alt", shift && "shift", shifted ?? key];
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
export function keyLabel(name: string, os: Os): string {
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
function chordLabel(chord: string, os: Os): string {
  const [first, second] = chord.split(" ") as [string, string];
  return `${keyLabel(first, os)} ${keyLabel(second, os)}`;
}

/** The key of `action` as the user reads it, such as "Ctrl+K O" for All Files or One File at a Time, or null
 * while the user has left it with no key. */
export function actionLabel(action: Action, os: Os): string | null {
  const key = keyOf(action);
  return key === null ? null : bindingLabel(key, os);
}

/** A key of one press or a chord as the user reads it: "ctrl+k o" is "Ctrl+K O", and "f7" is "F7". */
export function bindingLabel(key: string, os: Os): string {
  return isChord(key) ? chordLabel(key, os) : keyLabel(key, os);
}

/** `text` with the key of `action` after it, "Settings (Ctrl+,)", or alone while the action has no key. */
export function withKey(text: string, action: Action, os: Os): string {
  const label = actionLabel(action, os);
  return label === null ? text : `${text} (${label})`;
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

/** Whether a text box keeps press `name` on `target` for itself, as `typesInTextBox` says, whatever its
 * action. */
export function keptByTextBox(name: string, target: EventTarget | null): boolean {
  return typesInTextBox(name) && isTextBox(target);
}

/**
 * The handler for the window's capture phase. In a text box a key that types there, as `typesInTextBox`
 * says, is the box's, and any other key with a modifier is still Tsugiki's. Every build owns F12, which is
 * Go to definition: a development build opens DevTools on WebView2's own Ctrl+Shift+I, which Tsugiki leaves
 * alone (ADR 0013).
 */
export function keyGuard(bindings: Bindings, os: Os): (e: KeyboardEvent) => void {
  return (e) => {
    const name = keyName(e, os);
    if (name === null || !ownsKey(name) || keptByTextBox(name, e.target)) return;
    e.preventDefault();
    const action = actionOn(name);
    if (action !== undefined) bindings[action]?.(e);
  };
}
