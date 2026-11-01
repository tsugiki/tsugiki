// PROTOTYPE — throwaway. VS Code-style key handling: "ctrl+shift+p", "f7", "ctrl+k m" (chords), "?".
// On macOS Cmd counts as Ctrl, as in VS Code.
import { chord as chordUi } from "./chord.svelte";

export type Bindings = Record<string, (e: KeyboardEvent) => void>;

const typing = (e: KeyboardEvent) => {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
};

// With a non-Latin layout (Russian: Ctrl+P gives e.key "з") keys are named by their physical position,
// as in VS Code. Otherwise no binding matches and the browser's own Ctrl+P (Print) wins.
const CODE_KEYS: Record<string, string> = { Comma: ",", Period: ".", Slash: "/", Semicolon: ";", Quote: "'", BracketLeft: "[", BracketRight: "]", Backquote: "`", Minus: "-", Equal: "=", Backslash: "\\" };
const fromCode = (code: string) => /^Key[A-Z]$/.test(code) ? code[3].toLowerCase() : /^Digit\d$/.test(code) ? code[5] : CODE_KEYS[code];

/** A key press as a binding string: "ctrl+shift+p", "f7", "?". Null for a bare modifier. */
export function keyName(e: KeyboardEvent) {
  const k = e.key.length === 1 && /[^\x00-\x7f]/.test(e.key) ? (fromCode(e.code) ?? e.key.toLowerCase()) : e.key.toLowerCase();
  if (["control", "shift", "alt", "meta"].includes(k)) return null;
  const parts = [];
  if (e.ctrlKey || e.metaKey) parts.push("ctrl");
  if (e.altKey) parts.push("alt");
  if (e.shiftKey && k.length > 1) parts.push("shift"); // "?" already implies shift
  if (e.shiftKey && k.length === 1 && /[a-z]/.test(k)) parts.push("shift");
  parts.push(k === " " ? "space" : k);
  return parts.join("+");
}

/** Returns a keydown handler. Keys that work while typing must be listed in `whileTyping`. */
export function keymap(get: () => Bindings, whileTyping = ["escape", "ctrl+enter", "ctrl+p", "ctrl+shift+p", "ctrl+r", "ctrl+,"]) {
  let chord: string | null = null;
  let chordAt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // The status bar shows the first key of a chord until the second arrives or time runs out.
  const wait = (c: string | null) => {
    chord = c;
    chordUi.pending = c;
    clearTimeout(timer);
    if (c) timer = setTimeout(() => wait(null), 1500);
  };
  return (e: KeyboardEvent) => {
    const k = keyName(e);
    if (!k) return;
    const full = chord && Date.now() - chordAt < 1500 ? `${chord} ${k}` : k;
    const b = get();
    if (typing(e) && !whileTyping.includes(full)) return;
    if (b[full]) { e.preventDefault(); wait(null); b[full](e); return; }
    if (Object.keys(b).some((x) => x.startsWith(full + " "))) { e.preventDefault(); wait(full); chordAt = Date.now(); return; }
    wait(null);
  };
}

/** The keys from docs/scope.md, for the `?` overlay. */
export const KEY_HELP: [string, string][] = [
  ["Ctrl+Shift+P", "Command palette"],
  ["Ctrl+P", "Quick open: Issues, PRs, files in this PR"],
  ["↑ ↓ / Enter", "Move / open"],
  ["Esc", "Back / close"],
  ["F7 / Shift+F7", "Next / previous change"],
  ["F8 / Shift+F8", "Next / previous comment thread"],
  ["F12 / Alt+F12", "Go to / peek definition (stub)"],
  ["Ctrl+B", "Hide / show the Sidebar (as it was: full or icons)"],
  ["Ctrl+Alt+U", "Unified ↔ side-by-side"],
  ["Ctrl+Enter", "Send comment (to Pending review)"],
  ["Ctrl+Shift+E", "Open in editor (stub)"],
  ["Ctrl+K M", "Local mark"],
  ["Ctrl+K P", "Pin or unpin Issue"],
  ["E / M", "Archive / Mute the selected item"],
  ["Alt+← / Alt+→", "Back / forward"],
  ["Ctrl+Shift+G", "Open on GitHub"],
  ["Ctrl+R", "Switch Repository (also in the command palette)"],
  ["Ctrl+click", "Open a link in the browser"],
  ["?", "This list"],
];

const ARROWS: Record<string, string> = { arrowleft: "←", arrowright: "→", arrowup: "↑", arrowdown: "↓" };
/** "ctrl+shift+p" → "Ctrl+Shift+P", "ctrl+k m" → "Ctrl+K M", "alt+arrowleft" → "Alt+←". */
export const prettyKey = (k: string) =>
  k.split(" ").map((c) => c.split("+").map((p) => ARROWS[p] ?? (p.length === 1 ? p.toUpperCase() : p[0].toUpperCase() + p.slice(1))).join("+")).join(" ");
