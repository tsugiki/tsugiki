// Which rows of a diff the Files tab shows, and in what order. A collapsed file keeps its header and hides
// its other rows (#181), an expanded gap shows lines of the file between them (#182), the side-by-side view
// shows a change's removed rows beside its added ones (#183), and a review thread takes places under its
// line (#184), at the top of its file, under a line no hunk shows, or after the last file (#185). So the tab
// counts and places rows by where they show, and asks the core for the diff's rows by where they are in
// the diff.

import type { Gap, Pair, Rest } from "$lib/data/bindings";

/** The two views of a diff: each row under the one before, or the old lines beside the new. */
export type DiffView = "unified" | "side-by-side";

/** What shows at one place: a row of the diff, a line a gap left out, the buttons for lines a gap or the end
 * of a file still hides between lines that show, the button that shows more lines after a file's last
 * hunk, side by side a change's removed row and the added row beside it, of which one may be missing, the
 * heading of the threads on files no longer in the diff, or part of a review thread, `into` places down
 * from its top. */
export type Shown =
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | ({ kind: "hidden" } & HiddenLines)
  | { kind: "more"; file: number }
  | { kind: "pair"; removed: number; added: number | null }
  | { kind: "pair"; removed: null; added: number }
  | { kind: "gone" }
  | { kind: "thread"; id: string; into: number };

/** Lines of a file one after another, from line `from` at the head. */
export interface Lines {
  from: number;
  length: number;
}

/** Which lines the gaps and the ends of files show. */
export interface Expanded {
  /** The runs of `gap`'s lines in file `file` that show, in order. */
  gap(file: number, gap: Gap): readonly Lines[];
  /** The runs of lines after file `file`'s last hunk that show, from `rest` on, in order, and whether the
   * file ends with the last of them. */
  rest(file: number, rest: Rest): { runs: readonly Lines[]; ended: boolean };
}

/** A file as the layout needs it: its name, where its header is, the lines its hunks leave out, and its
 * changes. */
export interface LaidFile {
  path: string;
  row: number;
  gaps: readonly Gap[];
  rest: Rest | null;
  pairs: readonly Pair[];
}

/** Where a review thread sits: under a row of the diff, a file's header among them; under line `line` at
 * the head of file `file`, which no hunk shows; or after the last file. */
export type ThreadAnchor =
  { kind: "row"; row: number } | { kind: "line"; file: number; line: number } | { kind: "end" };

/** A review thread as the layout needs it: where it sits, how many places it takes, and its id, which names
 * it from one layout to the next. */
export interface LaidThread {
  at: ThreadAnchor;
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

/** Lines of file `file` that a gap or the end of the file hides, from line `old` at the merge base and
 * `new` at the head. */
export interface HiddenLines {
  file: number;
  old: number;
  new: number;
  lines: number;
}

/** A run of places that show one after another, from `shown` on. A pair's run starts `from` places into
 * its change, as a thread under one of its rows splits it. */
type Run<T> = { shown: number; length: number } & (
  | { kind: "row"; row: number }
  | { kind: "line"; file: number; old: number; new: number }
  | ({ kind: "hidden" } & HiddenLines)
  | { kind: "more"; file: number }
  | { kind: "pair"; row: number; removed: number; added: number; from: number }
  | { kind: "gone" }
  | { kind: "thread"; thread: number; laid: T }
);

/** A thread and its place among the threads. */
interface Waiting<T> {
  at: number;
  laid: T;
}

/** A part of a gap or of the end of a file: lines that show, or lines it still hides. */
interface Part {
  shown: boolean;
  from: number;
  length: number;
}

const nothing: Expanded = {
  gap: () => [],
  rest: () => ({ runs: [], ended: false }),
};

export class DiffLayout<T extends LaidThread = LaidThread> {
  /** How many places show. */
  readonly count: number;
  readonly view: DiffView;
  /** The commits the diff runs between, which tells a layout of another diff from one of the same. */
  readonly diff: string;
  readonly #rows: number;
  readonly #runs: Run<T>[] = [];
  readonly #laidFiles: readonly LaidFile[];
  readonly #files = new Map<number, number>();
  readonly #gaps = new Map<number, HiddenLines>();

  /** The layout of a diff `rows` long, whose files' headers sit at `files[i].row`, with the files that
   * `collapsed` names folded to their headers and the lines `expanded` names shown. A gap that shows every
   * line leaves out the row of its hunk, as its lines then run on into the hunk's; otherwise the hunk's row
   * stands for the hidden lines nearest it. Side by side, each change takes as many places as its longer
   * side. Each of `threads`, in the order they show, takes its places where it sits, unless a collapsed
   * file hides that; side by side, under the place that shows its row. A thread under a line that doesn't
   * show sits under the buttons that show it. */
  constructor(
    rows: number,
    files: readonly LaidFile[],
    collapsed: (file: number) => boolean,
    expanded: Expanded = nothing,
    view: DiffView = "unified",
    threads: readonly T[] = [],
    diff = "",
  ) {
    this.#rows = rows;
    this.view = view;
    this.diff = diff;
    this.#laidFiles = files;
    const underRows: (Waiting<T> & { row: number })[] = [];
    const underLines = new Map<number, (Waiting<T> & { line: number })[]>();
    const atEnd: Waiting<T>[] = [];
    threads.forEach((laid, at) => {
      if (laid.at.kind === "row") underRows.push({ at, laid, row: laid.at.row });
      else if (laid.at.kind === "end") atEnd.push({ at, laid });
      else {
        const file = underLines.get(laid.at.file) ?? [];
        file.push({ at, laid, line: laid.at.line });
        underLines.set(laid.at.file, file);
      }
    });
    let row = 0;
    // The changes of the file being laid out that show side by side, and the next to lay out.
    let pairs: readonly Pair[] = [];
    let next = 0;
    // The next thread under a row to lay out, and the row of the last one laid out.
    let waiting = 0;
    let under = -1;
    // The threads under lines of the file being laid out, and the next to lay out.
    let lineThreads: readonly (Waiting<T> & { line: number })[] = [];
    let nextLine = 0;
    const thread = ({ at, laid }: Waiting<T>) => {
      this.#add({ kind: "thread", thread: at, laid, shown: 0, length: laid.places });
    };
    const plainTo = (end: number) => {
      for (; ; waiting += 1) {
        const ahead = underRows[waiting];
        if (ahead === undefined || ahead.row >= end) break;
        // A collapsed file hid the row, unless the thread before sits under it too.
        if (ahead.row < row && ahead.row !== under) continue;
        if (ahead.row >= row) this.#add({ kind: "row", row, shown: 0, length: ahead.row + 1 - row });
        row = ahead.row + 1;
        under = ahead.row;
        thread(ahead);
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
        const placed: (Waiting<T> & { place: number })[] = [];
        for (; ; waiting += 1) {
          const ahead = underRows[waiting];
          if (ahead === undefined || ahead.row >= after) break;
          const into = ahead.row - pair.row;
          placed.push({ ...ahead, place: into < pair.removed ? into : into - pair.removed });
        }
        placed.sort((one, other) => one.place - other.place);
        const length = Math.max(pair.removed, pair.added);
        let from = 0;
        for (const ahead of placed) {
          if (ahead.place + 1 > from)
            this.#add({ kind: "pair", ...pair, from, shown: 0, length: ahead.place + 1 - from });
          from = ahead.place + 1;
          thread(ahead);
        }
        if (length > from) this.#add({ kind: "pair", ...pair, from, shown: 0, length: length - from });
        row = after;
      }
      plainTo(end);
    };
    // Lines `from` to `from + length` of file `file` at the head, each `shift` lines further on at the
    // merge base, with the threads under them.
    const lines = (file: number, shift: number, from: number, length: number) => {
      let start = from;
      const run = (end: number) => {
        if (end > start)
          this.#add({ kind: "line", file, old: start + shift, new: start, shown: 0, length: end - start });
        start = end;
      };
      for (; ; nextLine += 1) {
        const ahead = lineThreads[nextLine];
        if (ahead === undefined || ahead.line >= from + length) break;
        run(ahead.line + 1);
        thread(ahead);
      }
      run(from + length);
    };
    // The threads under lines before line `end` that don't show, which sit under the buttons that show them.
    const hiddenTo = (end: number) => {
      for (; ; nextLine += 1) {
        const ahead = lineThreads[nextLine];
        if (ahead === undefined || ahead.line >= end) break;
        thread(ahead);
      }
    };
    files.forEach((file, index) => {
      this.#files.set(file.row, index);
      const end = files[index + 1]?.row ?? rows;
      // The rows the file before left, with its pairs.
      rowsTo(file.row);
      const folded = collapsed(index);
      pairs = view === "side-by-side" && !folded ? file.pairs : [];
      next = 0;
      lineThreads = underLines.get(index) ?? [];
      nextLine = 0;
      if (folded) {
        // The threads at the top of the file go with its rows.
        this.#add({ kind: "row", row, shown: 0, length: 1 });
        row = end;
        return;
      }
      for (const gap of file.gaps) {
        rowsTo(gap.row);
        const shift = gap.old - gap.new;
        const parts = partsOf(gap.new, gap.new + gap.lines, expanded.gap(index, gap));
        const nearest = parts.findLast((part) => !part.shown);
        if (nearest !== undefined)
          this.#gaps.set(gap.row, {
            file: index,
            old: nearest.from + shift,
            new: nearest.from,
            lines: nearest.length,
          });
        for (const part of parts) {
          if (part.shown) {
            lines(index, shift, part.from, part.length);
            continue;
          }
          if (part === nearest) rowsTo(gap.row + 1);
          else this.#hidden({ file: index, old: part.from + shift, new: part.from, lines: part.length });
          hiddenTo(part.from + part.length);
        }
        row = gap.row + 1;
      }
      if (file.rest === null) return;
      rowsTo(end);
      const shift = file.rest.old - file.rest.new;
      const after = expanded.rest(index, file.rest);
      for (const part of partsOf(file.rest.new, Infinity, after.runs)) {
        if (part.shown) lines(index, shift, part.from, part.length);
        else if (part.length !== Infinity) {
          this.#hidden({ file: index, old: part.from + shift, new: part.from, lines: part.length });
          hiddenTo(part.from + part.length);
        }
      }
      if (!after.ended) this.#add({ kind: "more", file: index, shown: 0, length: 1 });
      hiddenTo(Infinity);
    });
    rowsTo(rows);
    if (atEnd.length > 0) this.#add({ kind: "gone", shown: 0, length: 1 });
    for (const ahead of atEnd) thread(ahead);
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

  /** Adds the buttons for `lines`, which a gap or the end of a file hides between lines that show. */
  #hidden(lines: HiddenLines) {
    this.#add({ kind: "hidden", ...lines, shown: 0, length: 1 });
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
      case "hidden":
        return { kind: "hidden", file: run.file, old: run.old, new: run.new, lines: run.lines };
      case "more":
        return { kind: "more", file: run.file };
      case "pair": {
        const place = run.from + into;
        const added = run.row + run.removed + place;
        if (place < run.removed)
          return { kind: "pair", removed: run.row + place, added: place < run.added ? added : null };
        return { kind: "pair", removed: null, added };
      }
      case "gone":
        return { kind: "gone" };
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

  /** The place that shows what place `place` of `from`, an earlier layout, shows: the same row, the row a
   * pair starts from, the same line, the same buttons, or as far into the same thread as fits. If none
   * does, `place` itself. A layout of another diff has other rows, so it shows the header of the file
   * `place` was in, if this diff has it. */
  moved(from: DiffLayout<T>, place: number): number {
    if (from.diff !== this.diff) {
      const path = from.#fileAt(place);
      const file = this.#laidFiles.find((file) => file.path === path);
      const header = file === undefined ? undefined : this.#placeOfRow(file.row);
      return header ?? place;
    }
    return this.#placeOf(from.at(place)) ?? place;
  }

  /** The name of the file that place `place` shows part of, if it shows part of one. */
  #fileAt(place: number): string | undefined {
    const run = this.#runAt(place);
    const row = (row: number) => this.#laidFiles.findLast((file) => file.row <= row)?.path;
    switch (run?.kind) {
      case undefined:
      case "gone":
        return undefined;
      case "row":
        return row(run.row + place - run.shown);
      case "pair":
        return row(run.row);
      case "line":
      case "hidden":
      case "more":
        return this.#laidFiles[run.file]?.path;
      case "thread": {
        const anchor = run.laid.at;
        if (anchor.kind === "row") return row(anchor.row);
        return anchor.kind === "line" ? this.#laidFiles[anchor.file]?.path : undefined;
      }
    }
  }

  /** The place that shows what `shown` names, if one does. */
  #placeOf(shown: Shown): number | undefined {
    switch (shown.kind) {
      case "row":
        return this.#placeOfRow(shown.row);
      case "pair":
        return this.#placeOfRow(shown.removed ?? shown.added);
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
      case "hidden":
        return this.#runs.find(
          (run) => run.kind === "hidden" && run.file === shown.file && run.new === shown.new,
        )?.shown;
      case "more":
        return this.#runs.find((run) => run.kind === "more" && run.file === shown.file)?.shown;
      case "gone":
        return this.#runs.find((run) => run.kind === "gone")?.shown;
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

  /** The lines the gap above the hunk whose row is `row` still hides nearest the hunk, if it hides any. */
  gapAt(row: number): HiddenLines | undefined {
    return this.#gaps.get(row);
  }
}

/** The parts of lines `from` to `end` at the head: the runs of `shown` that fall among them, in order, and
 * the lines between them that still hide. */
function partsOf(from: number, end: number, shown: readonly Lines[]): Part[] {
  const parts: Part[] = [];
  let at = from;
  for (const run of shown) {
    const start = Math.max(run.from, at);
    const stop = Math.min(run.from + run.length, end);
    if (stop <= start) continue;
    if (start > at) parts.push({ shown: false, from: at, length: start - at });
    parts.push({ shown: true, from: start, length: stop - start });
    at = stop;
  }
  if (end > at) parts.push({ shown: false, from: at, length: end - at });
  return parts;
}
