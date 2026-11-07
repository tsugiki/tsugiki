// Which rows of a diff the Files tab shows. A collapsed file keeps its header and hides its other rows, so
// the tab counts and places rows by where they show, and asks the core for them by where they are in the
// diff.

/** A run of rows that show one after another: from row `row` of the diff, the `length` rows that show from
 * `shown` on. */
interface Run {
  shown: number;
  row: number;
  length: number;
}

export class DiffLayout {
  /** How many rows show. */
  readonly count: number;
  readonly #runs: Run[] = [];
  readonly #files = new Map<number, number>();

  /** The layout of a diff `rows` long, whose files' headers sit at `files[i].row`, with the files that
   * `collapsed` names folded to their headers. */
  constructor(rows: number, files: readonly { row: number }[], collapsed: (file: number) => boolean) {
    let row = 0;
    let shown = 0;
    files.forEach((file, index) => {
      this.#files.set(file.row, index);
      if (!collapsed(index)) return;
      const end = files[index + 1]?.row ?? rows;
      this.#runs.push({ shown, row, length: file.row + 1 - row });
      shown += file.row + 1 - row;
      row = end;
    });
    this.#runs.push({ shown, row, length: rows - row });
    this.count = shown + rows - row;
  }

  /** The diff's row that shows at `index`. Past the end it counts on from the last row. */
  row(index: number): number {
    // The runs start in order, and the first at 0. A diff has few collapsed files, so this walks them.
    let row = index;
    for (const run of this.#runs) if (run.shown <= index) row = run.row + index - run.shown;
    return row;
  }

  /** The runs of the diff's rows that rows `first` to `last` show, each from its first row to the row after
   * its last. */
  pieces(first: number, last: number): [number, number][] {
    const end = Math.min(last, this.count - 1);
    const pieces: [number, number][] = [];
    for (const run of this.#runs) {
      const from = Math.max(first, run.shown);
      const to = Math.min(end, run.shown + run.length - 1);
      if (from <= to) pieces.push([run.row + from - run.shown, run.row + to - run.shown + 1]);
    }
    return pieces;
  }

  /** The file whose header is the diff's row `row`, if it is one. */
  fileAt(row: number): number | undefined {
    return this.#files.get(row);
  }
}
