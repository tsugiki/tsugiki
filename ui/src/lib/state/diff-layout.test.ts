import { describe, expect, it } from "vitest";
import type { Gap, Pair, Rest } from "$lib/data/bindings";
import {
  DiffLayout,
  type Expanded,
  type LaidFile,
  type LaidThread,
  type Lines,
  type ThreadAnchor,
} from "./diff-layout";
import type { Page } from "./file-pages";

/** Files whose headers sit at `rows`, with no gaps and no changes to pair. */
const at = (...rows: number[]): LaidFile[] =>
  rows.map((row) => ({ path: `f${String(row)}.py`, row, gaps: [], rest: null, pairs: [] }));

/** Places `first` to `last` of `layout`, as row numbers, `old/new` for a line, `hidden:new+lines` for the
 * buttons of hidden lines, `more:file`, `removed|added` for a pair, with `-` for a side that shows nothing,
 * `gone` for the heading of the threads on files no longer in the diff, and `tN` for thread N. */
const places = (layout: DiffLayout, first = 0, last = layout.count - 1) =>
  Array.from({ length: last - first + 1 }, (_, index) => {
    const shown = layout.at(first + index);
    if (shown.kind === "row") return shown.row;
    if (shown.kind === "line") return `${String(shown.old)}/${String(shown.new)}`;
    if (shown.kind === "pair") return `${String(shown.removed ?? "-")}|${String(shown.added ?? "-")}`;
    if (shown.kind === "thread") return shown.id;
    if (shown.kind === "hidden") return `hidden:${String(shown.new)}+${String(shown.lines)}`;
    if (shown.kind === "gone") return "gone";
    return `more:${String(shown.file)}`;
  });

/** Threads under rows `rows`, each taking `places` places. */
const threads = (places: number, ...rows: number[]): LaidThread[] =>
  rows.map((row, at) => ({ at: { kind: "row", row }, places, id: `t${String(at)}` }));

/** Threads that sit at `anchors`, each taking one place. */
const anchored = (...anchors: ThreadAnchor[]): LaidThread[] =>
  anchors.map((anchor, at) => ({ at: anchor, places: 1, id: `t${String(at)}` }));

/** A file at row 0 of 13 whose hunk at row 1 replaces 2 lines with 1 at rows 2 to 4, removes a line at
 * row 6, adds one at row 8, and replaces 1 line with 2 at rows 10 to 12. */
const changed: LaidFile = {
  path: "changed.py",
  row: 0,
  gaps: [],
  rest: null,
  pairs: [
    { row: 2, removed: 2, added: 1 },
    { row: 6, removed: 1, added: 0 },
    { row: 8, removed: 0, added: 1 },
    { row: 10, removed: 1, added: 2 },
  ] satisfies Pair[],
};

/** A file at row 0 whose two hunks start at rows 1 and 6, with 5 lines left out above the first and 30
 * above the second, which sit 2 lines further down at the head; and lines may follow the last hunk. */
const gapped = (rest: Rest | null = { old: 50, new: 52 }): LaidFile => ({
  path: "gapped.py",
  row: 0,
  gaps: [
    { row: 1, old: 1, new: 1, lines: 5 },
    { row: 6, old: 10, new: 12, lines: 30 },
  ] satisfies Gap[],
  rest,
  pairs: [],
});

/** Gaps opened as `gaps` says, by their hunk's row, from their top and from their bottom, and the lines
 * after each file's last hunk as `rests` says. */
const expanded = (
  gaps: Record<number, { top: number; bottom: number }>,
  rests: Record<number, { lines: number; ended: boolean }> = {},
): Expanded => ({
  gap: (_, gap) => {
    const { top, bottom } = gaps[gap.row] ?? { top: 0, bottom: 0 };
    return [
      { from: gap.new, length: top },
      { from: gap.new + gap.lines - bottom, length: bottom },
    ];
  },
  rest: (file, rest) => {
    const { lines, ended } = rests[file] ?? { lines: 0, ended: false };
    return { runs: [{ from: rest.new, length: lines }], ended };
  },
});

/** The runs of lines `runs` held in every file, the lines after a file's last hunk too, whose file ends
 * with the last run if `ended`. */
const holding = (runs: Lines[], ended = false): Expanded => ({
  gap: () => runs,
  rest: () => ({ runs, ended }),
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
    expect(layout.gapAt(6)).toEqual({ file: 0, old: 10, new: 12, lines: 30 });
    expect(layout.gapAt(2)).toBeUndefined();
    // The rows follow on around the button, so they ask as one.
    expect(layout.pieces(0, 99)).toEqual([[0, 10]]);
  });

  it("shows a gap's lines from its top below the hunk before, and from its bottom above its hunk", () => {
    const layout = new DiffLayout(8, [gapped(null)], () => false, expanded({ 6: { top: 2, bottom: 3 } }));

    // Rows 1 to 5 are the first hunk; row 6 heads the second, after 2 lines below the first and before 3
    // lines above its own.
    expect(places(layout)).toEqual([0, 1, 2, 3, 4, 5, "10/12", "11/13", 6, "37/39", "38/40", "39/41", 7]);
    expect(layout.gapAt(6)).toEqual({ file: 0, old: 12, new: 14, lines: 25 });
    expect(layout.pieces(0, 99)).toEqual([[0, 8]]);
    expect(layout.pieces(7, 9)).toEqual([[6, 7]]);
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

  it("shows each row at its own place in the unified view, a change's pairs included", () => {
    const layout = new DiffLayout(13, [changed], () => false);

    expect(places(layout)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
  });

  it("shows a change's removed rows beside its added ones side by side, as many places as the longer side", () => {
    const layout = new DiffLayout(13, [changed], () => false, undefined, "side-by-side");

    expect(places(layout)).toEqual([0, 1, "2|4", "3|-", 5, "6|-", 7, "-|8", 9, "10|11", "-|12"]);
    expect(layout.count).toBe(11);
    // A place asks for the rows it shows, of a change as much as of rows that follow on.
    expect(layout.pieces(3, 3)).toEqual([[3, 4]]);
    expect(layout.pieces(2, 2)).toEqual([
      [2, 3],
      [4, 5],
    ]);
    expect(layout.pieces(10, 10)).toEqual([[12, 13]]);
    expect(layout.pieces(0, 99)).toEqual([[0, 13]]);
  });

  it("pairs no rows of a collapsed file side by side", () => {
    const layout = new DiffLayout(15, [changed, ...at(13)], (file) => file === 0, undefined, "side-by-side");

    expect(places(layout)).toEqual([0, 13, 14]);
  });

  it("finds where a place of the other view shows, so switching views keeps the place", () => {
    // The second file is `gapped` 13 rows down, with a line showing after its last hunk.
    const second: LaidFile = {
      ...gapped(),
      row: 13,
      gaps: gapped().gaps.map((gap) => ({ ...gap, row: gap.row + 13 })),
    };
    const opened = expanded({}, { 1: { lines: 1, ended: false } });
    const unified = new DiffLayout(21, [changed, second], () => false, opened);
    const sideBySide = new DiffLayout(21, [changed, second], () => false, opened, "side-by-side");
    const there = (from: DiffLayout, to: DiffLayout, place: number) => to.moved(from, place);

    expect([0, 2, 3, 4, 5, 12].map((place) => there(unified, sideBySide, place))).toEqual([
      0, 2, 3, 2, 4, 10,
    ]);
    expect([2, 3, 9, 10].map((place) => there(sideBySide, unified, place))).toEqual([2, 3, 10, 12]);
    // The line after the second file's last hunk, and the button after it.
    expect([21, 22].map((place) => there(unified, sideBySide, place))).toEqual([19, 20]);
  });

  it("keeps the place where it was when the other view shows nothing there", () => {
    // The second file is `gapped` 13 rows down, with a line showing after its last hunk.
    const second: LaidFile = {
      ...gapped(),
      row: 13,
      gaps: gapped().gaps.map((gap) => ({ ...gap, row: gap.row + 13 })),
    };
    const opened = expanded({}, { 1: { lines: 1, ended: false } });
    const open = new DiffLayout(21, [changed, second], () => false, opened, "side-by-side");
    const collapsed = new DiffLayout(21, [changed, second], () => true, opened);

    // A row, a pair, the line after the second file's last hunk, and the button after it.
    expect([4, 10, 19, 20].map((place) => collapsed.moved(open, place))).toEqual([4, 10, 19, 20]);
  });

  it("shows each thread under its row, and asks for the rows around it as one", () => {
    const layout = new DiffLayout(6, at(0), () => false, undefined, "unified", threads(2, 2, 2, 4));

    expect(places(layout)).toEqual([0, 1, 2, "t0", "t0", "t1", "t1", 3, 4, "t2", "t2", 5]);
    expect(layout.at(4)).toEqual({ kind: "thread", id: "t0", into: 1 });
    expect(layout.pieces(0, 99)).toEqual([[0, 6]]);
    expect(layout.pieces(3, 4)).toEqual([]);
    expect(layout.threadsIn(4, 9)).toEqual([
      { thread: 0, laid: { at: { kind: "row", row: 2 }, places: 2, id: "t0" }, shown: 3, length: 2 },
      { thread: 1, laid: { at: { kind: "row", row: 2 }, places: 2, id: "t1" }, shown: 5, length: 2 },
      { thread: 2, laid: { at: { kind: "row", row: 4 }, places: 2, id: "t2" }, shown: 9, length: 2 },
    ]);
    expect(layout.threadsIn(7, 8)).toEqual([]);
    expect(["t0", "t2"].map((id) => layout.threadAt(id))).toEqual([3, 9]);
  });

  it("shows no thread under a row a collapsed file hides", () => {
    const layout = new DiffLayout(8, at(0, 4), (file) => file === 0, undefined, "unified", threads(1, 2, 5));

    expect(places(layout)).toEqual([0, 4, 5, "t1", 6, 7]);
    expect(layout.threadAt("t0")).toBeUndefined();
  });

  it("shows a thread side by side under the place that shows its row, splitting a change", () => {
    // Under the removed row 3, the added row 4 beside row 2, and the added row 12 at the change's end.
    const layout = new DiffLayout(13, [changed], () => false, undefined, "side-by-side", [
      { at: { kind: "row", row: 3 }, places: 1, id: "t0" },
      { at: { kind: "row", row: 4 }, places: 2, id: "t1" },
      { at: { kind: "row", row: 4 }, places: 1, id: "t2" },
      { at: { kind: "row", row: 12 }, places: 1, id: "t3" },
    ]);

    expect(places(layout)).toEqual([
      0,
      1,
      "2|4",
      "t1",
      "t1",
      "t2",
      "3|-",
      "t0",
      5,
      "6|-",
      7,
      "-|8",
      9,
      "10|11",
      "-|12",
      "t3",
    ]);
    expect(layout.pieces(6, 6)).toEqual([[3, 4]]);
    expect(layout.pieces(0, 99)).toEqual([[0, 13]]);
  });

  it("keeps the place in a thread, or as far into it as it still goes, as it grows or shrinks", () => {
    const tall = new DiffLayout(6, at(0), () => false, undefined, "unified", threads(4, 1));
    const short = new DiffLayout(6, at(0), () => false, undefined, "unified", threads(2, 1));
    const sideBySide = new DiffLayout(13, [changed], () => false, undefined, "side-by-side", threads(1, 3));
    const unified = new DiffLayout(13, [changed], () => false);

    // Row 3 below the thread, the thread's second place, and its fourth.
    expect([6, 3, 5].map((place) => short.moved(tall, place))).toEqual([4, 3, 3]);
    // Row 3 after its change's first place, row 5 after the thread, and the thread, which goes.
    expect([3, 5, 4].map((place) => unified.moved(sideBySide, place))).toEqual([3, 5, 4]);
    // Row 3 after a thread that splits its change, under row 2.
    const split = new DiffLayout(13, [changed], () => false, undefined, "side-by-side", threads(1, 2));
    expect(split.moved(unified, 3)).toBe(4);
  });

  it("shows a file's own threads under its header, and none of a collapsed file's", () => {
    const top = anchored({ kind: "row", row: 0 }, { kind: "row", row: 0 }, { kind: "row", row: 4 });

    const open = new DiffLayout(8, at(0, 4), () => false, undefined, "unified", top);
    const collapsed = new DiffLayout(8, at(0, 4), (file) => file === 0, undefined, "unified", top);

    expect(places(open)).toEqual([0, "t0", "t1", 1, 2, 3, 4, "t2", 5, 6, 7]);
    expect(places(collapsed)).toEqual([0, 4, "t2", 5, 6, 7]);
  });

  it("opens the lines around a thread in a gap, with buttons for those still hidden above and below", () => {
    const layout = new DiffLayout(
      8,
      [gapped(null)],
      () => false,
      holding([{ from: 20, length: 7 }]),
      "unified",
      anchored({ kind: "line", file: 0, line: 23 }),
    );

    // Lines 12 to 19 hide above the window, and 27 to 41 below it, nearest the hunk at row 6.
    expect(places(layout, 6)).toEqual([
      "hidden:12+8",
      "18/20",
      "19/21",
      "20/22",
      "21/23",
      "t0",
      "22/24",
      "23/25",
      "24/26",
      6,
      7,
    ]);
    expect(layout.gapAt(6)).toEqual({ file: 0, old: 25, new: 27, lines: 15 });
    expect(layout.at(6)).toEqual({ kind: "hidden", file: 0, old: 10, new: 12, lines: 8 });
  });

  it("puts a thread under a line that doesn't show under the buttons that show it", () => {
    const layout = new DiffLayout(
      8,
      [gapped()],
      () => false,
      holding([{ from: 30, length: 2 }]),
      "unified",
      anchored(
        { kind: "line", file: 0, line: 15 },
        { kind: "line", file: 0, line: 40 },
        { kind: "line", file: 0, line: 90 },
      ),
    );

    expect(places(layout, 6)).toEqual(["hidden:12+18", "t0", "28/30", "29/31", 6, "t1", 7, "more:0", "t2"]);
  });

  it("opens the lines around a thread after a file's last hunk, and the buttons for those between", () => {
    const thread = anchored({ kind: "line", file: 0, line: 58 });
    const runs = [
      { from: 52, length: 2 },
      { from: 57, length: 3 },
    ];

    const more = new DiffLayout(8, [gapped()], () => false, holding(runs), "unified", thread);
    const ended = new DiffLayout(8, [gapped()], () => false, holding(runs, true), "unified", thread);

    expect(places(more, 8)).toEqual([
      "50/52",
      "51/53",
      "hidden:54+3",
      "55/57",
      "56/58",
      "t0",
      "57/59",
      "more:0",
    ]);
    expect(places(ended, 8)).toEqual(["50/52", "51/53", "hidden:54+3", "55/57", "56/58", "t0", "57/59"]);
  });

  it("puts the threads on files no longer in the diff under a heading after the last file", () => {
    const layout = new DiffLayout(
      3,
      at(0),
      () => false,
      undefined,
      "unified",
      anchored({ kind: "end" }, { kind: "row", row: 1 }, { kind: "end" }),
    );

    expect(places(layout)).toEqual([0, 1, "t1", 2, "gone", "t0", "t2"]);
    expect(layout.moved(layout, 4)).toBe(4);
  });

  it("finds the same buttons in a layout of the same diff, as more lines show", () => {
    const before = new DiffLayout(8, [gapped()], () => false, holding([{ from: 30, length: 2 }]));
    const after = new DiffLayout(8, [gapped()], () => false, holding([{ from: 30, length: 4 }]));

    // The buttons of lines 12 to 29, and the hunk's row, which stands for the lines after the window.
    expect([6, 9].map((place) => after.moved(before, place))).toEqual([6, 11]);
    expect(after.at(9)).toEqual({ kind: "line", file: 0, old: 30, new: 32 });
  });

  it("shows the header of the file that was at the top when another diff takes the layout's place", () => {
    // `gapped` moves to the top, `changed` follows it, and a third file goes.
    const second: LaidFile = {
      ...gapped(),
      row: 13,
      gaps: gapped().gaps.map((gap) => ({ ...gap, row: gap.row + 13 })),
    };
    const before = new DiffLayout(
      23,
      [changed, second, ...at(21)],
      () => false,
      holding([{ from: 20, length: 2 }]),
      "side-by-side",
      anchored(
        { kind: "row", row: 3 },
        { kind: "line", file: 1, line: 21 },
        { kind: "row", row: 22 },
        { kind: "end" },
      ),
      "base..old",
    );
    const after = new DiffLayout<LaidThread>(
      21,
      [gapped(), { ...changed, row: 8 }],
      () => false,
      undefined,
      "unified",
      [],
      "base..new",
    );
    const kinds = Array.from({ length: before.count }, (_, place) => before.at(place).kind);
    const first = (kind: string) => kinds.indexOf(kind as (typeof kinds)[number]);

    // Two pairs and a thread of `changed`, one of them with no removed row; a line, its thread and the
    // buttons of `gapped`, and the button after its last hunk.
    expect(
      [2, 7, first("thread"), first("line"), first("line") + 2, first("hidden"), first("more")].map((place) =>
        after.moved(before, place),
      ),
    ).toEqual([9, 9, 9, 0, 0, 0, 0]);
    const gone = first("gone");
    // A row of the third file, a thread under it, the heading of the threads on no file, one of them, and a
    // place past the end.
    const tried = [gone - 3, gone - 1, gone, gone + 1, gone + 9];
    expect(tried.map((place) => after.moved(before, place))).toEqual(tried);
  });

  it("shows nothing for an empty diff", () => {
    const layout = new DiffLayout(0, [], () => false);

    expect(layout.count).toBe(0);
    expect(layout.pieces(0, 10)).toEqual([]);
  });

  describe("one page at a time", () => {
    /** Three files at rows 0, 5 and 9 of 12, with threads under rows 2 and 6, and one on a file no longer
     * in the diff. */
    const files = at(0, 5, 9);
    const laid = anchored({ kind: "row", row: 2 }, { kind: "row", row: 6 }, { kind: "end" });
    const page = (only: Page, collapsed: (file: number) => boolean = () => false) =>
      new DiffLayout(12, files, collapsed, undefined, "unified", laid, "", only);

    it("shows only one file's rows, with its own threads", () => {
      const layout = page({ kind: "file", file: 1 });

      expect(places(layout)).toEqual([5, 6, "t1", 7, 8]);
      expect(layout.pieces(0, layout.count - 1)).toEqual([[5, 9]]);
      expect(layout.fileAt(5)).toBe(1);
      expect(layout.fileAt(0)).toBeUndefined();
    });

    it("shows the last file to the diff's end", () => {
      expect(places(page({ kind: "file", file: 2 }))).toEqual([9, 10, 11]);
    });

    it("shows only the header of a collapsed file", () => {
      expect(places(page({ kind: "file", file: 1 }, (file) => file === 1))).toEqual([5]);
    });

    it("shows the threads on files no longer in the diff on a page of their own", () => {
      expect(places(page({ kind: "gone" }))).toEqual(["gone", "t2"]);
    });

    it("shows no rows on the page of NEWS entries, nor for a file the diff doesn't have", () => {
      expect(page({ kind: "news" }).count).toBe(0);
      expect(page({ kind: "file", file: 3 }).count).toBe(0);
    });

    it("keeps the place as the tab switches between all files and one", () => {
      const all = new DiffLayout(12, files, () => false, undefined, "unified", laid);
      const one = page({ kind: "file", file: 1 });
      const sixth = all.placeOfRow(6) ?? -1;

      expect(one.moved(all, sixth)).toBe(1);
      expect(all.moved(one, 1)).toBe(sixth);
      expect(all.moved(one, 2)).toBe(all.threadAt("t1"));
    });
  });

  it("names the page each place belongs to, so the tab can show the one at the top alone", () => {
    const layout = new DiffLayout(
      12,
      at(0, 5, 9),
      () => false,
      undefined,
      "unified",
      anchored({ kind: "row", row: 2 }, { kind: "end" }),
    );
    const pageAt = (place: number) => layout.pageAt(place);

    expect([
      layout.headerOf({ kind: "file", file: 1 }),
      layout.headerOf({ kind: "file", file: 5 }),
      layout.headerOf({ kind: "gone" }),
      layout.headerOf({ kind: "news" }),
    ]).toEqual([layout.placeOfRow(5), undefined, layout.count - 2, undefined]);

    expect(pageAt(0)).toEqual({ kind: "file", file: 0 });
    expect(pageAt(layout.threadAt("t0") ?? -1)).toEqual({ kind: "file", file: 0 });
    expect(pageAt(layout.placeOfRow(7) ?? -1)).toEqual({ kind: "file", file: 1 });
    expect(pageAt(layout.placeOfRow(11) ?? -1)).toEqual({ kind: "file", file: 2 });
    expect(pageAt(layout.threadAt("t1") ?? -1)).toEqual({ kind: "gone" });
    expect(pageAt(layout.count - 2)).toEqual({ kind: "gone" });
    expect(pageAt(layout.count + 5)).toBeUndefined();
  });
});
