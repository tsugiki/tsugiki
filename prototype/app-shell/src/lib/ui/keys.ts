// PROTOTYPE — throwaway. VS Code-style key handling: "ctrl+shift+p", "f7", "ctrl+k m" (chords), "?".
// On macOS Cmd counts as Ctrl, as in VS Code.
export type Bindings = Record<string, (e: KeyboardEvent) => void>;

const typing = (e: KeyboardEvent) => {
  const t = e.target as HTMLElement | null;
  return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
};

function name(e: KeyboardEvent) {
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase();
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
export function keymap(get: () => Bindings, whileTyping = ["escape", "ctrl+enter", "ctrl+p", "ctrl+shift+p", "ctrl+r"]) {
  let chord: string | null = null;
  let chordAt = 0;
  return (e: KeyboardEvent) => {
    const k = name(e);
    if (!k) return;
    const full = chord && Date.now() - chordAt < 1500 ? `${chord} ${k}` : k;
    const b = get();
    if (typing(e) && !whileTyping.includes(full)) return;
    if (b[full]) { e.preventDefault(); chord = null; b[full](e); return; }
    if (Object.keys(b).some((x) => x.startsWith(full + " "))) { e.preventDefault(); chord = full; chordAt = Date.now(); return; }
    chord = null;
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
  ["Ctrl+B", "Toggle sidebar"],
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
