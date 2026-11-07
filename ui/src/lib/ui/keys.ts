// Tsugiki's keys (ADR 0013). The window's guard runs before anything else sees a key, and stops the browser
// for every key Tsugiki owns, so a key with no action yet can't print or reload instead.

export type Os = "mac" | "other";

/**
 * The keys from `docs/scope.md`, named as `keyName` names them. `ctrl+k` starts the chords `Ctrl+K M` and
 * `Ctrl+K P`, and most of these have no action until their stage arrives.
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
  "ctrl+alt+u",
  "ctrl+pagedown",
  "ctrl+pageup",
  "ctrl+enter",
  "ctrl+shift+e",
  "ctrl+k",
  "alt+arrowleft",
  "alt+arrowright",
  "ctrl+shift+g",
  "ctrl+r",
  "e",
  "m",
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

const MODIFIERS = new Set(["control", "shift", "alt", "meta"]);

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
 * A key press as a binding: "ctrl+shift+p", "f7", "?". On macOS Cmd is called ctrl, as `docs/scope.md` has
 * it. Null for a key that can't be a binding: a modifier alone, AltGr, which types a character but reports
 * Ctrl and Alt, and the Windows key or macOS's Ctrl, which belong to the OS and to text boxes.
 */
export function keyName(e: KeyboardEvent, os: Os): string | null {
  const [primary, foreign] = os === "mac" ? [e.metaKey, e.ctrlKey] : [e.ctrlKey, e.metaKey];
  const byPlace = goesByPlace(e.key);
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
 * The handler for the window's capture phase. In a text box a plain key types, and a key with a modifier is
 * still Tsugiki's. A key in `forBrowser` goes through, and has no action.
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
      (name.length === 1 && isTextBox(e.target))
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
