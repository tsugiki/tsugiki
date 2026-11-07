// Which rows of a diff the Files tab shows, and in what order. A collapsed file keeps its header and hides
// its other rows (#181), and an expanded gap shows lines of the file between them (#182), so the tab counts
// and places rows by where they show, and asks the core for the diff's rows by where they are in the diff.

import type { Gap, Rest } from "$lib/data/bindings";

/** What shows at one place: a row of the diff, a line a gap left out, or the button that shows the lines
 * after a file's last hunk. */
export type Shown =
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | { kind: "more"; file: number };

/** How much of each gap the user has expanded. */
export interface Expanded {
  /** The lines of `gap` in file `file` that show from its top and from its bottom. */
  gap(file: number, gap: Gap): { top: number; bottom: number };
  /** The lines that show after file `file`'s last hunk, from `rest` on, and whether the file ends there. */
  rest(file: number, rest: Rest): { lines: number; ended: boolean };
}

/** A file as the layout needs it: where its header is, and the lines its hunks leave out. */
export interface LaidFile {
  row: number;
  gaps: readonly Gap[];
  rest: Rest | null;
}

/** A gap above a hunk that still hides lines, and the file it's in. */
export interface HiddenGap {
  file: number;
  gap: Gap;
  /** How many of its lines don't show. */
  hidden: number;
}

/** A run of places that show one after another, from `shown` on. */
type Run = { shown: number; length: number } & (
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | { kind: "more"; file: number }
);

const nothing: Expanded = {
  gap: () => ({ top: 0, bottom: 0 }),
  rest: () => ({ lines: 0, ended: false }),
};

export class DiffLayout {
  /** How many places show. */
  readonly count: number;
  readonly #rows: number;
  readonly #runs: Run[] = [];
  readonly #files = new Map<number, number>();
  readonly #gaps = new Map<number, HiddenGap>();

  /** The layout of a diff `rows` long, whose files' headers sit at `files[i].row`, with the files that
   * `collapsed` names folded to their headers and the gaps `expanded` names opened. A gap opened all the
   * way leaves out the row of its hunk, as its lines then run on into the hunk's. */
  constructor(
    rows: number,
    files: readonly LaidFile[],
    collapsed: (file: number) => boolean,
    expanded: Expanded = nothing,
  ) {
    this.#rows = rows;
    let row = 0;
    const rowsTo = (end: number) => {
      if (end > row) this.#add({ kind: "row", row, shown: 0, length: end - row });
      row = end;
    };
    const lines = (file: number, old: number, line: number, length: number) => {
      if (length > 0) this.#add({ kind: "line", file, old, new: line, shown: 0, length });
    };
    files.forEach((file, index) => {
      this.#files.set(file.row, index);
      const end = files[index + 1]?.row ?? rows;
      if (collapsed(index)) {
        rowsTo(file.row + 1);
        row = end;
        return;
      }
      for (const gap of file.gaps) {
        const opened = expanded.gap(index, gap);
        const top = Math.min(opened.top, gap.lines);
        const bottom = Math.min(opened.bottom, gap.lines - top);
        const hidden = gap.lines - top - bottom;
        if (hidden > 0) this.#gaps.set(gap.row, { file: index, gap, hidden });
        rowsTo(gap.row);
        lines(index, gap.old, gap.new, top);
        if (hidden > 0) rowsTo(gap.row + 1);
        row = gap.row + 1;
        const from = gap.lines - bottom;
        lines(index, gap.old + from, gap.new + from, bottom);
      }
      if (file.rest === null) return;
      rowsTo(end);
      const after = expanded.rest(index, file.rest);
      lines(index, file.rest.old, file.rest.new, after.lines);
      if (!after.ended) this.#add({ kind: "more", file: index, shown: 0, length: 1 });
    });
    rowsTo(rows);
    const last = this.#runs.at(-1);
    this.count = last === undefined ? 0 : last.shown + last.length;
  }

  /** Adds `run` after the others, as one with the run before when both are rows that follow on. */
  #add(run: Run) {
    const last = this.#runs.at(-1);
    const shown = last === undefined ? 0 : last.shown + last.length;
    if (last?.kind === "row" && run.kind === "row" && last.row + last.length === run.row) {
      last.length += run.length;
      return;
    }
    this.#runs.push({ ...run, shown });
  }

  /** The run that place `index` is in, or nothing outside the places. A file the diff goes on past adds a
   * run, so this halves the runs, which start in order. */
  #runAt(index: number): Run | undefined {
    let low = 0;
    let high = this.#runs.length;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (this.#runs.slice(middle, middle + 1).every((run) => run.shown <= index)) low = middle + 1;
      else high = middle;
    }
    // The last run that starts at or before `index`.
    const [run] = this.#runs.slice(low - 1, low);
    return run !== undefined && index < run.shown + run.length ? run : undefined;
  }

  /** What shows at place `index`. Past the end it counts on from the diff's last row. */
  at(index: number): Shown {
    const run = this.#runAt(index);
    const into = run === undefined ? 0 : index - run.shown;
    switch (run?.kind) {
      case undefined:
        return { kind: "row", row: this.#rows + index - this.count };
      case "row":
        return { kind: "row", row: run.row + into };
      case "line":
        return { kind: "line", file: run.file, old: run.old + into, new: run.new + into };
      case "more":
        return { kind: "more", file: run.file };
    }
  }

  /** The runs of the diff's rows that places `first` to `last` show, each from its first row to the row
   * after its last. */
  pieces(first: number, last: number): [number, number][] {
    const end = Math.min(last, this.count - 1);
    const pieces: [number, number][] = [];
    for (const run of this.#runs) {
      if (run.kind !== "row") continue;
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

  /** The gap above the hunk whose row is `row`, if it still hides lines. */
  gapAt(row: number): HiddenGap | undefined {
    return this.#gaps.get(row);
  }
}
