// Which rows of a diff the Files tab shows, and in what order. A collapsed file keeps its header and hides
// its other rows (#181), an expanded gap shows lines of the file between them (#182), the side-by-side view
// shows a change's removed rows beside its added ones (#183), and a review thread takes places under its
// line (#184). So the tab counts and places rows by where they show, and asks the core for the diff's rows
// by where they are in the diff.

import type { Gap, Pair, Rest } from "$lib/data/bindings";

/** The two views of a diff: each row under the one before, or the old lines beside the new. */
export type DiffView = "unified" | "side-by-side";

/** What shows at one place: a row of the diff, a line a gap left out, the button that shows the lines
 * after a file's last hunk, side by side a change's removed row and the added row beside it, of which one
 * may be missing, or part of a review thread, `into` places down from its top. */
export type Shown =
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | { kind: "more"; file: number }
  | { kind: "pair"; removed: number; added: number | null }
  | { kind: "pair"; removed: null; added: number }
  | { kind: "thread"; id: string; into: number };

/** How much of each gap the user has expanded. */
export interface Expanded {
  /** The lines of `gap` in file `file` that show from its top and from its bottom. */
  gap(file: number, gap: Gap): { top: number; bottom: number };
  /** The lines that show after file `file`'s last hunk, from `rest` on, and whether the file ends there. */
  rest(file: number, rest: Rest): { lines: number; ended: boolean };
}

/** A file as the layout needs it: where its header is, the lines its hunks leave out, and its changes. */
export interface LaidFile {
  row: number;
  gaps: readonly Gap[];
  rest: Rest | null;
  pairs: readonly Pair[];
}

/** A review thread as the layout needs it: the row it sits under, how many places it takes, and its id,
 * which names it from one layout to the next. */
export interface LaidThread {
  row: number;
  places: number;
  id: string;
}

/** The places a review thread takes: `thread` is its place among the threads, and `laid` the thread as the
 * layout was given it. */
export interface ThreadPlaces<T extends LaidThread> {
  thread: number;
  laid: T;
  shown: number;
  length: number;
}

/** A gap above a hunk that still hides lines, and the file it's in. */
export interface HiddenGap {
  file: number;
  gap: Gap;
  /** How many of its lines don't show. */
  hidden: number;
}

/** A run of places that show one after another, from `shown` on. A pair's run starts `from` places into
 * its change, as a thread under one of its rows splits it. */
type Run<T> = { shown: number; length: number } & (
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | { kind: "more"; file: number }
  | { kind: "pair"; row: number; removed: number; added: number; from: number }
  | { kind: "thread"; thread: number; laid: T }
);

const nothing: Expanded = {
  gap: () => ({ top: 0, bottom: 0 }),
  rest: () => ({ lines: 0, ended: false }),
};

export class DiffLayout<T extends LaidThread = LaidThread> {
  /** How many places show. */
  readonly count: number;
  readonly view: DiffView;
  readonly #rows: number;
  readonly #runs: Run<T>[] = [];
  readonly #files = new Map<number, number>();
  readonly #gaps = new Map<number, HiddenGap>();

  /** The layout of a diff `rows` long, whose files' headers sit at `files[i].row`, with the files that
   * `collapsed` names folded to their headers and the gaps `expanded` names opened. A gap opened all the
   * way leaves out the row of its hunk, as its lines then run on into the hunk's. Side by side, each change
   * takes as many places as its longer side. Each of `threads`, in the order of their rows, takes its places
   * under its row, unless a collapsed file hides the row; side by side, under the place that shows it. */
  constructor(
    rows: number,
    files: readonly LaidFile[],
    collapsed: (file: number) => boolean,
    expanded: Expanded = nothing,
    view: DiffView = "unified",
    threads: readonly T[] = [],
  ) {
    this.#rows = rows;
    this.view = view;
    let row = 0;
    // The changes of the file being laid out that show side by side, and the next to lay out.
    let pairs: readonly Pair[] = [];
    let next = 0;
    // The next thread to lay out, and the row of the last one laid out.
    let waiting = 0;
    let under = -1;
    const thread = (at: number, laid: T) => {
      this.#add({ kind: "thread", thread: at, laid, shown: 0, length: laid.places });
    };
    const plainTo = (end: number) => {
      for (; ; waiting += 1) {
        const ahead = threads[waiting];
        if (ahead === undefined || ahead.row >= end) break;
        // A collapsed file hid the row, unless the thread before sits under it too.
        if (ahead.row < row && ahead.row !== under) continue;
        if (ahead.row >= row) this.#add({ kind: "row", row, shown: 0, length: ahead.row + 1 - row });
        row = ahead.row + 1;
        under = ahead.row;
        thread(waiting, ahead);
      }
      if (end > row) this.#add({ kind: "row", row, shown: 0, length: end - row });
      row = end;
    };
    const rowsTo = (end: number) => {
      for (; ; next += 1) {
        const pair = pairs[next];
        if (pair === undefined || pair.row >= end) break;
        plainTo(pair.row);
        const after = pair.row + pair.removed + pair.added;
        // The threads under the change's rows, by the place that shows each row.
        const under: { at: number; place: number; laid: T }[] = [];
        for (; ; waiting += 1) {
          const ahead = threads[waiting];
          if (ahead === undefined || ahead.row >= after) break;
          const into = ahead.row - pair.row;
          const place = into < pair.removed ? into : into - pair.removed;
          under.push({ at: waiting, place, laid: ahead });
        }
        under.sort((one, other) => one.place - other.place);
        const length = Math.max(pair.removed, pair.added);
        let from = 0;
        for (const { at, place, laid } of under) {
          if (place + 1 > from)
            this.#add({ kind: "pair", ...pair, from, shown: 0, length: place + 1 - from });
          from = place + 1;
          thread(at, laid);
        }
        if (length > from) this.#add({ kind: "pair", ...pair, from, shown: 0, length: length - from });
        row = after;
      }
      plainTo(end);
    };
    const lines = (file: number, old: number, line: number, length: number) => {
      if (length > 0) this.#add({ kind: "line", file, old, new: line, shown: 0, length });
    };
    files.forEach((file, index) => {
      this.#files.set(file.row, index);
      const end = files[index + 1]?.row ?? rows;
      // The rows the file before left, with its pairs.
      rowsTo(file.row);
      const folded = collapsed(index);
      pairs = view === "side-by-side" && !folded ? file.pairs : [];
      next = 0;
      if (folded) {
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
  #add(run: Run<T>) {
    const last = this.#runs.at(-1);
    const shown = last === undefined ? 0 : last.shown + last.length;
    if (last?.kind === "row" && run.kind === "row" && last.row + last.length === run.row) {
      last.length += run.length;
      return;
    }
    this.#runs.push({ ...run, shown });
  }

  /** How many runs start at or before place `index`. Each gap and pair can add a run, so this halves the
   * runs, which start in order. */
  #starting(index: number): number {
    let low = 0;
    let high = this.#runs.length;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (this.#runs.slice(middle, middle + 1).every((run) => run.shown <= index)) low = middle + 1;
      else high = middle;
    }
    return low;
  }

  /** The run that place `index` is in, or nothing outside the places. */
  #runAt(index: number): Run<T> | undefined {
    const low = this.#starting(index);
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
      case "pair": {
        const place = run.from + into;
        const added = run.row + run.removed + place;
        if (place < run.removed)
          return { kind: "pair", removed: run.row + place, added: place < run.added ? added : null };
        return { kind: "pair", removed: null, added };
      }
      case "thread":
        return { kind: "thread", id: run.laid.id, into };
    }
  }

  /** The threads that take any of places `first` to `last`. */
  threadsIn(first: number, last: number): ThreadPlaces<T>[] {
    const threads: ThreadPlaces<T>[] = [];
    for (const run of this.#runs.slice(Math.max(0, this.#starting(first) - 1))) {
      if (run.shown > last) break;
      if (run.kind === "thread" && run.shown + run.length > first)
        threads.push({ thread: run.thread, laid: run.laid, shown: run.shown, length: run.length });
    }
    return threads;
  }

  /** The first place thread `id` takes, if it shows. */
  threadAt(id: string): number | undefined {
    return this.#runs.find((run) => run.kind === "thread" && run.laid.id === id)?.shown;
  }

  /** The runs of the diff's rows that places `first` to `last` show, each from its first row to the row
   * after its last, in order. A change's places show rows from both its sides, and a thread can split a
   * change, so its rows come out of order and get sorted. */
  pieces(first: number, last: number): [number, number][] {
    const end = Math.min(last, this.count - 1);
    const found: [number, number][] = [];
    const add = (from: number, to: number) => {
      if (from < to) found.push([from, to]);
    };
    for (const run of this.#runs.slice(Math.max(0, this.#starting(first) - 1))) {
      if (run.shown > end) break;
      if (run.kind !== "row" && run.kind !== "pair") continue;
      // The first and last places it shows, counted from its own first, and a pair's from its change's.
      const skipped = run.kind === "pair" ? run.from : 0;
      const from = Math.max(first, run.shown) - run.shown + skipped;
      const to = Math.min(end, run.shown + run.length - 1) - run.shown + 1 + skipped;
      if (run.kind === "row") {
        add(run.row + from, run.row + to);
        continue;
      }
      add(run.row + from, run.row + Math.min(to, run.removed));
      const added = run.row + run.removed;
      add(added + from, added + Math.min(to, run.added));
    }
    found.sort((one, other) => one[0] - other[0]);
    const pieces: [number, number][] = [];
    for (const [from, to] of found) {
      const before = pieces.at(-1);
      if (before !== undefined && before[1] >= from) before[1] = Math.max(before[1], to);
      else pieces.push([from, to]);
    }
    return pieces;
  }

  /** The place that shows what place `place` of `from` shows, an earlier layout: the same row, the row a
   * pair starts from, the same line, the same button, or as far into the same thread as fits. If none
   * does, `place` itself. */
  moved(from: DiffLayout<T>, place: number): number {
    return this.#placeOf(from.at(place)) ?? place;
  }

  /** The place that shows what `shown` names, if one does. */
  #placeOf(shown: Shown): number | undefined {
    switch (shown.kind) {
      case "row":
        return this.#placeOfRow(shown.row);
      case "pair":
        return this.#placeOfRow(shown.removed === null ? shown.added : shown.removed);
      case "line": {
        const run = this.#runs.find(
          (run) =>
            run.kind === "line" &&
            run.file === shown.file &&
            run.new <= shown.new &&
            shown.new < run.new + run.length,
        );
        return run?.kind === "line" ? run.shown + shown.new - run.new : undefined;
      }
      case "more":
        return this.#runs.find((run) => run.kind === "more" && run.file === shown.file)?.shown;
      case "thread": {
        const run = this.#runs.find((run) => run.kind === "thread" && run.laid.id === shown.id);
        return run === undefined ? undefined : run.shown + Math.min(shown.into, run.length - 1);
      }
    }
  }

  /** The place that shows the diff's row `row`, if one does. */
  #placeOfRow(row: number): number | undefined {
    for (const run of this.#runs) {
      if (run.kind === "row" && run.row <= row && row < run.row + run.length)
        return run.shown + row - run.row;
      if (run.kind === "pair" && run.row <= row && row < run.row + run.removed + run.added) {
        const into = row - run.row;
        const place = into < run.removed ? into : into - run.removed;
        // An earlier run of the same change holds the places before `from`.
        if (place < run.from + run.length) return run.shown + place - run.from;
      }
    }
    return undefined;
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
