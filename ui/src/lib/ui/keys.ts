// Tsugiki's keys (ADR 0013). The window's guard runs before anything else sees a key, and stops the browser
// for every key Tsugiki owns, so a key with no action yet can't print or reload instead. Only the wait for a
// chord's second key comes first: the key that ends it goes to neither.

export type Os = "mac" | "other";

/**
 * The keys from `docs/devel/keyboard.md`, named as `keyName` names them. `ctrl+k` starts the chords in
 * `OWNED_CHORDS`, and most of these have no action until their stage arrives. The bare ← and → that fold
 * the Issue list stay out: the guard would take them from every text box, diff and pane edge, and the list
 * acts on them only while it has the focus.
 */
const OWNED_KEYS = [
  "ctrl+shift+p",
  "ctrl+shift+h",
  "ctrl+p",
  "f12",
  "alt+f12",
  "f7",
  "shift+f7",
  "f8",
  "shift+f8",
  "ctrl+f",
  "ctrl+b",
  "ctrl+alt+b",
  "ctrl+alt+u",
  "ctrl+pagedown",
  "ctrl+pageup",
  "alt+pagedown",
  "alt+pageup",
  "ctrl+enter",
  "ctrl+shift+e",
  "ctrl+k",
  "alt+arrowleft",
  "alt+arrowright",
  "ctrl+shift+g",
  "ctrl+r",
  "e",
  "m",
  "q",
  "shift+q",
  "ctrl+,",
  "ctrl+=",
  "ctrl+-",
  "ctrl+0",
  "?",
] as const;

export type OwnedKey = (typeof OWNED_KEYS)[number];

/** What a bound key does. */
export type Bindings = Readonly<Partial<Record<OwnedKey, () => void>>>;

const OWNED: ReadonlySet<string> = new Set(OWNED_KEYS);

function isOwned(name: string): name is OwnedKey {
  return OWNED.has(name);
}

/** The first key of each chord. */
const CHORD_STARTS = ["ctrl+k"] as const satisfies readonly OwnedKey[];

export type ChordStart = (typeof CHORD_STARTS)[number];

const STARTS: ReadonlySet<string> = new Set(CHORD_STARTS);

export function isChordStart(name: string): name is ChordStart {
  return STARTS.has(name);
}

/** The chords from `docs/devel/keyboard.md`, named as `chordName` names them. Each has no action until its
 * stage: the screen that owns one binds it, as `RepositoryHome` binds `ctrl+k z` and the page `ctrl+k t`. */
const OWNED_CHORDS = [
  "ctrl+k j",
  "ctrl+k m",
  "ctrl+k o",
  "ctrl+k p",
  "ctrl+k r",
  "ctrl+k t",
  "ctrl+k z",
] as const satisfies readonly `${ChordStart} ${string}`[];

export type OwnedChord = (typeof OWNED_CHORDS)[number];

/** What a bound chord does. */
export type ChordBindings = Readonly<Partial<Record<OwnedChord, () => void>>>;

const CHORDS: ReadonlySet<string> = new Set(OWNED_CHORDS);

export function isOwnedChord(name: string): name is OwnedChord {
  return CHORDS.has(name);
}

/**
 * A chord as a binding: its two presses, each named by `keyName`, joined by a space, such as "ctrl+k z". The
 * Settings Keyboard page shows and remaps a chord by this name.
 */
export function chordName(first: ChordStart, second: string): string {
  return `${first} ${second}`;
}

/** Whether `name` types in a text box: a plain key, a capital letter such as Shift+Q, or Ctrl+Alt+B, which
 * AltGr sends too on Windows. */
function typesInTextBox(name: OwnedKey): boolean {
  return /^(?:shift\+)?.$/.test(name) || name === "ctrl+alt+b";
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
export function chordLabel(chord: OwnedChord, os: Os): string {
  const [first, second] = chord.split(" ") as [ChordStart, string];
  return `${keyLabel(first, os)} ${second.toUpperCase()}`;
}

/** Which OS this is, for `keyName`. */
export function currentOs(platform: string = navigator.platform): Os {
  return platform.startsWith("Mac") ? "mac" : "other";
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
 * key with a modifier is still Tsugiki's. A key in `forBrowser` goes through, and has no action.
 */
export function keyGuard(
  bindings: Bindings,
  os: Os,
  forBrowser: ReadonlySet<OwnedKey> = new Set(),
): (e: KeyboardEvent) => void {
  return (e) => {
    const name = keyName(e, os);
    if (
      name === null ||
      !isOwned(name) ||
      forBrowser.has(name) ||
      (typesInTextBox(name) && isTextBox(e.target))
    ) {
      return;
    }
    e.preventDefault();
    bindings[name]?.();
  };
}

/**
 * What a build leaves to the browser. `preventDefault` holds back the engine's shortcuts as well as the
 * page's, so owning F12 would stop a development build opening DevTools (ADR 0013).
 */
export function keysForBrowser(isDevelopment: boolean): ReadonlySet<OwnedKey> {
  return new Set<OwnedKey>(isDevelopment ? ["f12"] : []);
}
