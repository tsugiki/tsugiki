// A diff row's text cut at the spans the core sent for it (#178), at the search's matches (#186), and at the
// word the user picked and its caret (#437).
import type { Picked } from "$lib/state/pick";
import type { Highlight, Span } from "$lib/state/pr-files.svelte";

/** A match of the search in the diff (#186), from `start` to `end` in UTF-16 code units, and whether the tab
 * is on it. */
export type Match = { start: number; end: number; current: boolean };

/** A run of a row's text with one look. `picked` marks the word the user picked, and `caret` a caret
 * before the run; a caret at the end of the text comes before an empty run. Each is there only when true. */
export type Piece = {
  text: string;
  highlight: Highlight | null;
  found: "match" | "current" | null;
  picked?: true;
  caret?: true;
};

/** `text` in pieces: each span's text with its highlight, and the plain text between them, cut again where
 * the search's matches `found` start and end, and where the word `picked` and its caret are. The spans, the
 * matches and the pick count UTF-16 code units, as JavaScript's strings do, and the spans and the matches
 * each come in order without overlapping. */
export function pieces(text: string, spans: Span[], found: Match[] = [], picked?: Picked): Piece[] {
  // Most rows hold no match and no pick, and the rows drawn while scrolling take this way.
  if (found.length === 0 && picked === undefined) return highlighted(text, spans);
  const caret = picked?.caret ?? undefined;
  const runs = picked === undefined ? [...spans, ...found] : [...spans, ...found, picked];
  const edges = runs.flatMap((run) => [run.start, run.end]);
  const cuts = [...new Set([0, text.length, ...edges, ...(caret === undefined ? [] : [caret])])];
  cuts.sort((one, other) => one - other);
  const out: Piece[] = [];
  for (const [at, start] of cuts.entries()) {
    const end = cuts[at + 1];
    if (end === undefined) continue;
    const span = spans.find((span) => span.start <= start && start < span.end);
    const match = found.find((match) => match.start <= start && start < match.end);
    const piece: Piece = {
      text: text.slice(start, end),
      highlight: span?.highlight ?? null,
      found: match === undefined ? null : match.current ? "current" : "match",
    };
    if (picked !== undefined && picked.start <= start && start < picked.end) piece.picked = true;
    if (caret === start) piece.caret = true;
    out.push(piece);
  }
  if (caret === text.length) out.push({ text: "", highlight: null, found: null, caret: true });
  return out;
}

/** `text` in pieces at its spans alone. */
function highlighted(text: string, spans: Span[]): Piece[] {
  const out: Piece[] = [];
  let at = 0;
  for (const span of spans) {
    if (span.start > at) out.push({ text: text.slice(at, span.start), highlight: null, found: null });
    out.push({ text: text.slice(span.start, span.end), highlight: span.highlight, found: null });
    at = span.end;
  }
  if (at < text.length) out.push({ text: text.slice(at), highlight: null, found: null });
  return out;
}
