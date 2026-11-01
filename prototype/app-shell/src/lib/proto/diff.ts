// PROTOTYPE — throwaway. Unified diff -> rows, with per-line highlight.js colouring.
// The real renderer is the one validated on branch prototype/diff-renderer (rows in Rust, syntect);
// this one only has to look and feel right on small PRs.
import hljs from "highlight.js/lib/core";
import c from "highlight.js/lib/languages/c";
import python from "highlight.js/lib/languages/python";
import plaintext from "highlight.js/lib/languages/plaintext";
import type { Detail, Thread } from "./model.svelte";

hljs.registerLanguage("c", c);
hljs.registerLanguage("python", python);
hljs.registerLanguage("plaintext", plaintext);

export type RowKind = "hunk" | "ctx" | "add" | "del";
export interface Row { kind: RowKind; o: number | null; n: number | null; html: string; text: string }
export interface DiffFile {
  path: string; adds: number; dels: number; generated: boolean; news: boolean;
  rows: Row[]; threads: Thread[]; outdated: Thread[];
}

const lang = (path: string) => (/\.(c|h)$/.test(path) ? "c" : /\.py$/.test(path) ? "python" : "plaintext");
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

export function parseDiff(d: Detail): DiffFile[] {
  const files: DiffFile[] = [];
  let f: DiffFile | null = null;
  let o = 0, n = 0, language = "plaintext";
  for (const line of d.diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      const path = line.slice(line.indexOf(" b/") + 3);
      const meta = d.files.find((x) => x.path === path);
      f = { path, adds: 0, dels: 0, generated: !!meta?.generated, news: path.startsWith("Misc/NEWS.d/"), rows: [], threads: [], outdated: [] };
      files.push(f);
      language = lang(path);
      continue;
    }
    if (!f || line.startsWith("+++") || line.startsWith("---") || line.startsWith("index ") || /^(new|deleted) file|^similarity|^rename/.test(line)) continue;
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/.exec(line);
    if (h) {
      o = +h[1]; n = +h[2];
      f.rows.push({ kind: "hunk", o: null, n: null, html: esc(line), text: line });
      continue;
    }
    const sign = line[0], text = line.slice(1);
    if (sign !== "+" && sign !== "-" && sign !== " ") continue;
    const html = language === "plaintext" ? esc(text) : hljs.highlight(text, { language, ignoreIllegals: true }).value;
    if (sign === "+") { f.rows.push({ kind: "add", o: null, n: n++, html, text }); f.adds++; }
    else if (sign === "-") { f.rows.push({ kind: "del", o: o++, n: null, html, text }); f.dels++; }
    else f.rows.push({ kind: "ctx", o: o++, n: n++, html, text });
  }
  for (const t of d.threads) {
    const file = files.find((x) => x.path === t.path);
    if (!file) continue;
    (t.outdated ? file.outdated : file.threads).push(t);
  }
  // NEWS entries first, Generated files last: the order a reviewer reads in.
  return files.sort((a, b) => +b.news - +a.news || +a.generated - +b.generated);
}

/** Which row a thread sits under: the new-side line (RIGHT) or the old-side line (LEFT). */
export function rowOfThread(f: DiffFile, t: Thread): number {
  const want = t.line;
  const i = f.rows.findIndex((r) => (t.side === "LEFT" ? r.o === want : r.n === want) && r.kind !== "hunk");
  return i < 0 ? f.rows.length - 1 : i;
}

export const newsText = (f: DiffFile) => f.rows.filter((r) => r.kind === "add").map((r) => r.text).join("\n");
