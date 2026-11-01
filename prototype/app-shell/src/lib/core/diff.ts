// PROTOTYPE — throwaway. Core: unified diff -> rows per file, threads placed on rows. Plain text only;
// syntax highlighting is a UI concern (ui/highlight.ts). The real renderer is the one validated on
// branch prototype/diff-renderer (rows in Rust, syntect).
import type { Detail, Thread } from "./types";

export type RowKind = "hunk" | "ctx" | "add" | "del";
export interface Row { kind: RowKind; o: number | null; n: number | null; text: string }
export interface DiffFile {
  path: string; adds: number; dels: number; generated: boolean; news: boolean;
  rows: Row[]; threads: Thread[]; outdated: Thread[];
}

export function parseDiff(d: Detail): DiffFile[] {
  const files: DiffFile[] = [];
  let f: DiffFile | null = null;
  let o = 0, n = 0;
  for (const line of d.diff.split("\n")) {
    if (line.startsWith("diff --git ")) {
      const path = line.slice(line.indexOf(" b/") + 3);
      const meta = d.files.find((x) => x.path === path);
      f = { path, adds: 0, dels: 0, generated: !!meta?.generated, news: path.startsWith("Misc/NEWS.d/"), rows: [], threads: [], outdated: [] };
      files.push(f);
      continue;
    }
    if (!f || line.startsWith("+++") || line.startsWith("---") || line.startsWith("index ") || /^(new|deleted) file|^similarity|^rename/.test(line)) continue;
    const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)/.exec(line);
    if (h) {
      o = +h[1]; n = +h[2];
      f.rows.push({ kind: "hunk", o: null, n: null, text: line });
      continue;
    }
    const sign = line[0], text = line.slice(1);
    if (sign === "+") { f.rows.push({ kind: "add", o: null, n: n++, text }); f.adds++; }
    else if (sign === "-") { f.rows.push({ kind: "del", o: o++, n: null, text }); f.dels++; }
    else if (sign === " ") f.rows.push({ kind: "ctx", o: o++, n: n++, text });
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

// --- Expanding context, as on GitHub ------------------------------------------------------------
// The lines between hunks are unchanged, so the head version of the file is all that is needed: a
// hidden line's old-side number is its new-side number minus the shift at that gap.

/** Lines revealed in a gap: `down` from its top (below the previous hunk), `up` from its bottom. */
export interface Shown { down: number; up: number }
/** A gap's control row: the hunk header it stands for (empty after the last hunk) and what's hidden. */
export interface GapInfo { i: number; hidden: number | null; first: boolean; last: boolean }
export type ViewRow =
  | (Row & { x?: true })
  | { kind: "gap"; o: null; n: null; text: string; gap: GapInfo };

export const EXPAND_STEP = 20;

interface Hunk { at: number; o: number; n: number; oEnd: number; nEnd: number }
function hunksOf(rows: Row[]): Hunk[] {
  const out: Hunk[] = [];
  rows.forEach((r, i) => {
    if (r.kind === "hunk") {
      const h = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(r.text)!;
      out.push({ at: i, o: +h[1], n: +h[2], oEnd: +h[1], nEnd: +h[2] });
    } else {
      const h = out.at(-1)!;
      if (r.o != null) h.oEnd = r.o + 1;
      if (r.n != null) h.nEnd = r.n + 1;
    }
  });
  return out;
}

/** Gap k precedes hunk k; the last one follows the last hunk. `end` is null while the file isn't loaded. */
export function gapsOf(f: DiffFile, total: number | null) {
  const hs = hunksOf(f.rows);
  return hs.map((h, k) => ({ start: k ? hs[k - 1].nEnd : 1, end: h.n - 1, shift: h.n - h.o }))
    .concat(hs.length ? [{ start: hs.at(-1)!.nEnd, end: total ?? Infinity, shift: hs.at(-1)!.nEnd - hs.at(-1)!.oEnd }] : []);
}

/**
 * The rows to draw: the diff with each gap's revealed lines (marked `x`) and, where lines are still
 * hidden, a control row. `src` is the file at the PR's head, or null while it loads (or when there is
 * none: a deleted file).
 */
export function withContext(f: DiffFile, src: string[] | null, shown: Record<number, Shown>): ViewRow[] {
  const hs = hunksOf(f.rows);
  const gaps = gapsOf(f, src ? src.length : null);
  const out: ViewRow[] = [];
  const reveal = (from: number, to: number, shift: number) => {
    for (let n = from; n <= to && src; n++) out.push({ kind: "ctx", o: n - shift, n, text: src[n - 1] ?? "", x: true });
  };
  gaps.forEach((g, k) => {
    const last = k === hs.length;
    const size = Number.isFinite(g.end) ? Math.max(0, g.end - g.start + 1) : null;
    const s = src ? shown[k] ?? { down: 0, up: 0 } : { down: 0, up: 0 };
    const down = Math.min(s.down, size ?? s.down), up = last ? 0 : Math.min(s.up, (size ?? 0) - down);
    const hidden = size == null ? null : size - down - up;
    reveal(g.start, g.start + down - 1, g.shift);
    const header = last ? "" : f.rows[hs[k].at].text;
    // A gap that was never there (the first hunk starts at line 1) keeps its header; one opened all
    // the way merges into the context around it, as on GitHub.
    if (hidden !== 0 || (size === 0 && !last)) out.push({ kind: "gap", o: null, n: null, text: header, gap: { i: k, hidden, first: k === 0, last } });
    if (!last) {
      reveal(g.end - up + 1, g.end, g.shift);
      for (let i = hs[k].at + 1; i < (hs[k + 1]?.at ?? f.rows.length); i++) out.push(f.rows[i]);
    }
  });
  return out;
}

/** The threads under each drawn row: a thread sits under its line in the diff, or the last row. */
export function threadRows(view: ViewRow[], threads: Thread[]): Map<number, Thread[]> {
  const at = new Map<number, Thread[]>();
  for (const t of threads) {
    let i = view.findIndex((r) => r.kind !== "gap" && r.kind !== "hunk" && !("x" in r && r.x) && (t.side === "LEFT" ? r.o === t.line : r.n === t.line));
    if (i < 0) i = view.length - 1;
    at.set(i, [...(at.get(i) ?? []), t]);
  }
  return at;
}
