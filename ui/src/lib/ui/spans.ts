// A diff row's text cut at the spans the core sent for it (#178), and at the search's matches (#186).
import type { Highlight, Span } from "$lib/state/pr-files.svelte";

/** A match of the search in the diff (#186), from `start` to `end` in UTF-16 code units, and whether the tab
 * is on it. */
export type Match = { start: number; end: number; current: boolean };

export type Piece = { text: string; highlight: Highlight | null; found: "match" | "current" | null };

/** `text` in pieces: each span's text with its highlight, and the plain text between them, cut again where
 * the search's matches `found` start and end. The spans and the matches count UTF-16 code units, as
 * JavaScript's strings do, and each come in order without overlapping. */
export function pieces(text: string, spans: Span[], found: Match[] = []): Piece[] {
  // Most rows hold no match, and the rows drawn while scrolling take this way.
  if (found.length === 0) return highlighted(text, spans);
  const cuts = [...new Set([0, text.length, ...[...spans, ...found].flatMap((run) => [run.start, run.end])])];
  cuts.sort((one, other) => one - other);
  const out: Piece[] = [];
  for (const [at, start] of cuts.entries()) {
    const end = cuts[at + 1];
    if (end === undefined) continue;
    const span = spans.find((span) => span.start <= start && start < span.end);
    const match = found.find((match) => match.start <= start && start < match.end);
    out.push({
      text: text.slice(start, end),
      highlight: span?.highlight ?? null,
      found: match === undefined ? null : match.current ? "current" : "match",
    });
  }
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
