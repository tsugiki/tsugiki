import { describe, expect, it } from "vitest";
import type { Gap, Rest } from "$lib/data/bindings";
import { DiffLayout, type Expanded, type LaidFile } from "./diff-layout";

/** Files whose headers sit at `rows`, with no gaps. */
const at = (...rows: number[]): LaidFile[] => rows.map((row) => ({ row, gaps: [], rest: null }));

/** What places `first` to `last` show, as row numbers, `old/new` for a line, and `more:file`. */
const places = (layout: DiffLayout, first = 0, last = layout.count - 1) =>
  Array.from({ length: last - first + 1 }, (_, index) => {
    const shown = layout.at(first + index);
    if (shown.kind === "row") return shown.row;
    if (shown.kind === "line") return `${String(shown.old)}/${String(shown.new)}`;
    return `more:${String(shown.file)}`;
  });

/** A file at row 0 whose two hunks start at rows 1 and 6, with 5 lines left out above the first and 30
 * above the second, which sit 2 lines further down at the head; and lines may follow the last hunk. */
const gapped = (rest: Rest | null = { old: 50, new: 52 }): LaidFile => ({
  row: 0,
  gaps: [
    { row: 1, old: 1, new: 1, lines: 5 },
    { row: 6, old: 10, new: 12, lines: 30 },
  ] satisfies Gap[],
  rest,
});

/** Gaps opened as `gaps` says, by their hunk's row, and the lines after each file's last hunk as `rests`
 * says. */
const expanded = (
  gaps: Record<number, { top: number; bottom: number }>,
  rests: Record<number, { lines: number; ended: boolean }> = {},
): Expanded => ({
  gap: (_, gap) => gaps[gap.row] ?? { top: 0, bottom: 0 },
  rest: (file) => rests[file] ?? { lines: 0, ended: false },
});

describe("DiffLayout", () => {
  it("shows every row when no file is collapsed", () => {
    const layout = new DiffLayout(10, at(0, 4), () => false);

    expect(layout.count).toBe(10);
    expect([0, 4, 9].map((index) => layout.at(index))).toEqual([
      { kind: "row", row: 0 },
      { kind: "row", row: 4 },
      { kind: "row", row: 9 },
    ]);
    expect(layout.pieces(2, 7)).toEqual([[2, 8]]);
  });

  it("keeps a collapsed file's header and hides its rows", () => {
    // Files at rows 0, 4 and 7 of 10; the second is collapsed, so rows 5 and 6 go.
    const layout = new DiffLayout(10, at(0, 4, 7), (file) => file === 1);

    expect(layout.count).toBe(8);
    expect(places(layout)).toEqual([0, 1, 2, 3, 4, 7, 8, 9]);
    expect(layout.pieces(3, 6)).toEqual([
      [3, 5],
      [7, 9],
    ]);
  });

  it("hides the last file's rows to the diff's end", () => {
    const layout = new DiffLayout(10, at(0, 4), (file) => file === 1);

    expect(layout.count).toBe(5);
    expect(layout.at(4)).toEqual({ kind: "row", row: 4 });
    expect(layout.pieces(0, 99)).toEqual([[0, 5]]);
  });

  it("covers nothing for rows past its end or a range that runs backwards", () => {
    const layout = new DiffLayout(10, at(0), () => false);

    expect(layout.pieces(10, 20)).toEqual([]);
    expect(layout.pieces(5, 4)).toEqual([]);
    expect(layout.at(10)).toEqual({ kind: "row", row: 10 });
    expect(layout.at(-1)).toEqual({ kind: "row", row: -1 });
  });

  it("finds the file whose header a row is", () => {
    const layout = new DiffLayout(10, at(0, 4, 7), () => false);

    expect([0, 4, 7, 5].map((row) => layout.fileAt(row))).toEqual([0, 1, 2, undefined]);
  });

  it("shows the button for the lines after a file's last hunk, and the gaps hide their lines", () => {
    const layout = new DiffLayout(10, [gapped(), ...at(8)], () => false);

    expect(places(layout)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, "more:0", 8, 9]);
    expect(layout.gapAt(6)).toEqual({ file: 0, gap: gapped().gaps[1], hidden: 30 });
    expect(layout.gapAt(2)).toBeUndefined();
    expect(layout.pieces(0, 99)).toEqual([
      [0, 8],
      [8, 10],
    ]);
  });

  it("shows a gap's lines from its top below the hunk before, and from its bottom above its hunk", () => {
    const layout = new DiffLayout(8, [gapped(null)], () => false, expanded({ 6: { top: 2, bottom: 3 } }));

    // Rows 1 to 5 are the first hunk; row 6 heads the second, after 2 lines below the first and before 3
    // lines above its own.
    expect(places(layout)).toEqual([0, 1, 2, 3, 4, 5, "10/12", "11/13", 6, "37/39", "38/40", "39/41", 7]);
    expect(layout.gapAt(6)?.hidden).toBe(25);
    expect(layout.pieces(0, 99)).toEqual([
      [0, 6],
      [6, 7],
      [7, 8],
    ]);
  });

  it("leaves out a hunk's row once its gap shows every line, and never shows a line twice", () => {
    const layout = new DiffLayout(8, [gapped(null)], () => false, expanded({ 1: { top: 4, bottom: 9 } }));

    expect(places(layout, 0, 6)).toEqual([0, "1/1", "2/2", "3/3", "4/4", "5/5", 2]);
    expect(layout.gapAt(1)).toBeUndefined();
  });

  it("shows the lines after a file's last hunk, and no button once the file ends", () => {
    const more = new DiffLayout(8, [gapped()], () => false, expanded({}, { 0: { lines: 2, ended: false } }));
    const ended = new DiffLayout(8, [gapped()], () => false, expanded({}, { 0: { lines: 2, ended: true } }));

    expect(places(more, 8)).toEqual(["50/52", "51/53", "more:0"]);
    expect(places(ended, 8)).toEqual(["50/52", "51/53"]);
  });

  it("shows neither gaps nor the button of a collapsed file", () => {
    const layout = new DiffLayout(
      10,
      [gapped(), ...at(8)],
      (file) => file === 0,
      expanded({ 1: { top: 5, bottom: 0 } }),
    );

    expect(places(layout)).toEqual([0, 8, 9]);
    expect(layout.gapAt(1)).toBeUndefined();
  });

  it("shows nothing for an empty diff", () => {
    const layout = new DiffLayout(0, [], () => false);

    expect(layout.count).toBe(0);
    expect(layout.pieces(0, 10)).toEqual([]);
  });
});
