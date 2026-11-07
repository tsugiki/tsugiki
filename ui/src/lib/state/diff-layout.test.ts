import { describe, expect, it } from "vitest";
import { DiffLayout } from "./diff-layout";

/** Files whose headers sit at `rows`, the diff `total` rows long. */
const at = (...rows: number[]) => rows.map((row) => ({ row }));

describe("DiffLayout", () => {
  it("shows every row when no file is collapsed", () => {
    const layout = new DiffLayout(10, at(0, 4), () => false);

    expect(layout.count).toBe(10);
    expect([0, 4, 9].map((index) => layout.row(index))).toEqual([0, 4, 9]);
    expect(layout.pieces(2, 7)).toEqual([[2, 8]]);
  });

  it("keeps a collapsed file's header and hides its rows", () => {
    // Files at rows 0, 4 and 7 of 10; the second is collapsed, so rows 5 and 6 go.
    const layout = new DiffLayout(10, at(0, 4, 7), (file) => file === 1);

    expect(layout.count).toBe(8);
    expect(Array.from({ length: 8 }, (_, index) => layout.row(index))).toEqual([0, 1, 2, 3, 4, 7, 8, 9]);
    expect(layout.pieces(3, 6)).toEqual([
      [3, 5],
      [7, 9],
    ]);
  });

  it("hides the last file's rows to the diff's end", () => {
    const layout = new DiffLayout(10, at(0, 4), (file) => file === 1);

    expect(layout.count).toBe(5);
    expect(layout.row(4)).toBe(4);
    expect(layout.pieces(0, 99)).toEqual([[0, 5]]);
  });

  it("covers nothing for rows past its end or a range that runs backwards", () => {
    const layout = new DiffLayout(10, at(0), () => false);

    expect(layout.pieces(10, 20)).toEqual([]);
    expect(layout.pieces(5, 4)).toEqual([]);
    expect(layout.row(10)).toBe(10);
  });

  it("finds the file whose header a row is", () => {
    const layout = new DiffLayout(10, at(0, 4, 7), () => false);

    expect([0, 4, 7, 5].map((row) => layout.fileAt(row))).toEqual([0, 1, 2, undefined]);
  });
});
