// The word the user picks in the diff with a click or a double-click (#437), which Go to definition and the
// Definition peek look up.

/** A line of code in the diff that takes a pick: row `row` of the diff, or line `line` at the head of the
 * file `file` among the diff's files, shown by expanding a gap. */
export type PickedLine = { kind: "row"; row: number } | { kind: "line"; file: number; line: number };

/** The half of a side-by-side row a line shows on, as a line of context shows on both. Unified, null. */
export type Column = "left" | "right" | null;

/** The commit a line is at: the PR's base for a removed line, its head for the rest. */
export type Side = "base" | "head";

/** The pick's place in a line's text: its word from `start` to `end` in UTF-16 code units, and the caret, or
 * null once a double-click selected the word. */
export type Picked = { start: number; end: number; caret: number | null };

/** The word the user picked, and its place in its line's text. `file`, `side` and `line` say where a lookup
 * finds it: the file's place among the diff's files, the commit, and the line's number at that commit. */
export type Pick = Picked & {
  at: PickedLine;
  column: Column;
  file: number;
  side: Side;
  line: number;
  word: string;
};

const WORD = /[\p{L}\p{M}\p{N}_]+/gu;

/** The identifier around offset `offset` of `text`, in UTF-16 code units: letters, digits and underscores.
 * An offset at either end of a word is on it. */
export function wordAt(text: string, offset: number): { start: number; end: number } | undefined {
  for (const match of text.matchAll(WORD)) {
    const end = match.index + match[0].length;
    if (match.index > offset) return undefined;
    if (offset <= end) return { start: match.index, end };
  }
  return undefined;
}
