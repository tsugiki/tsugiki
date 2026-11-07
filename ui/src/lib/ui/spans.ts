// A diff row's text cut at the spans the core sent for it (#178).
import type { Highlight, Span } from "$lib/state/pr-files.svelte";

export type Piece = { text: string; highlight: Highlight | null };

/** `text` in pieces: each span's text with its highlight, and the plain text between them. The spans count
 * UTF-16 code units, as JavaScript's strings do, and come in order without overlapping. */
export function pieces(text: string, spans: Span[]): Piece[] {
  const out: Piece[] = [];
  let at = 0;
  for (const span of spans) {
    if (span.start > at) out.push({ text: text.slice(at, span.start), highlight: null });
    out.push({ text: text.slice(span.start, span.end), highlight: span.highlight });
    at = span.end;
  }
  if (at < text.length) out.push({ text: text.slice(at), highlight: null });
  return out;
}
