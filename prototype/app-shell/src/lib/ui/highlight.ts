// PROTOTYPE — throwaway. UI: per-line highlight.js colouring for core's diff rows.
import hljs from "highlight.js/lib/core";
import c from "highlight.js/lib/languages/c";
import python from "highlight.js/lib/languages/python";
import type { DiffFile } from "$lib/core/diff";

hljs.registerLanguage("c", c);
hljs.registerLanguage("python", python);

const lang = (path: string) => (/\.(c|h)$/.test(path) ? "c" : /\.py$/.test(path) ? "python" : null);
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** HTML for each row of a file, same order as `f.rows`. */
export function highlightRows(f: DiffFile): string[] {
  const language = lang(f.path);
  return f.rows.map((r) => (r.kind === "hunk" || !language ? esc(r.text) : hljs.highlight(r.text, { language, ignoreIllegals: true }).value));
}
