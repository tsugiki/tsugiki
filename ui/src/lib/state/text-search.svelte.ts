// The rail's Search tab (#686): a query over the code of the PR on show, at the head of the diff its Files
// tab shows, as the core's text search runs it in the Lookup checkout (#685). The checkout follows that
// diff, so a query waits until the diff is ready, and runs again on a newer one. Typing runs the query once
// the keys pause, and each toggle runs it at once. The core numbers each query and says when what it found
// may have changed, and this shows only what answers the query it asked last. The results group by file,
// and the keys move a cursor through the rows, fold and unfold a file, and open a match.
import { SvelteSet } from "svelte/reactivity";
import {
  commands,
  events,
  type End,
  type FileHits,
  type LineHit,
  type Query,
  type SearchState,
} from "$lib/data/bindings";

export type { FileHits, LineHit, SearchState };

/** How long the keys pause before the query runs, in milliseconds. */
export const SEARCH_PAUSE = 250;

/** The options of a query, each on a toggle beside the box and a key while the box has the focus. */
export type SearchOption = "case" | "words" | "regex";

/** A row of the results: a file, with how many matches it holds and whether it is folded, or a line of it
 * that holds a match. */
export type SearchRow =
  | { kind: "file"; file: FileHits; matches: number; folded: boolean }
  | { kind: "line"; file: FileHits; hit: LineHit };

/** A match to open: the file's path and the line's number, at the head of the diff on show. */
export type Match = { path: string; line: number };

/** What the tab shows under the box. */
type Shown =
  | { kind: "empty" }
  | { kind: "waiting" }
  | { kind: "bad_query"; message: string }
  | { kind: "problem"; message: string }
  | { kind: "no_checkout" }
  | { kind: "failed" }
  | { kind: "results"; files: FileHits[]; matches: number; end: End };

/** A row by what it shows, so the cursor stays on it as results come in above it. */
type RowKey = { path: string; line: number | undefined };

const counted = (count: number, one: string, many: string) =>
  `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;

/** `text` as a regex that matches it as it is, as VS Code puts the picked text in a box with regex on. */
export function literalRegex(text: string): string {
  return text.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
}

export class TextSearchState {
  /** What the box holds. */
  text = $state("");
  matchCase = $state(false);
  wholeWord = $state(false);
  regex = $state(false);
  /** Whether the box should take the focus, as `Ctrl+Shift+F` asks. The box takes it once it shows. */
  wantsFocus = $state(false);
  #shown = $state.raw<Shown>({ kind: "empty" });
  #folded = new SvelteSet<string>();
  #cursor = $state.raw<RowKey>();
  readonly #repository: string;
  /** The head of the diff on show, once it is ready. Until then the checkout may be another PR's. */
  #head: string | undefined;
  /** The core's number for the query asked last, once it has answered. */
  #id: number | undefined;
  /** How many queries this has asked, so a late answer to an older one changes nothing. */
  #asked = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #listening: Promise<() => void> | undefined;
  #reading = false;
  /** The core said something changed while a read was under way, which may have missed it. */
  #again = false;
  #stopped = false;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /** The box's text as the user types it: the query runs once the keys pause. */
  type(text: string) {
    this.text = text;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.#run(), SEARCH_PAUSE);
  }

  /** Turns `option` on or off, and runs the query at once. */
  toggle(option: SearchOption) {
    if (option === "case") this.matchCase = !this.matchCase;
    else if (option === "words") this.wholeWord = !this.wholeWord;
    else this.regex = !this.regex;
    void this.#run();
  }

  /** Puts `text`, the picked word or the selected text, in the box, as a regex matches it with regex on, and
   * runs the query at once. */
  seed(text: string) {
    this.text = this.regex ? literalRegex(text) : text;
    void this.#run();
  }

  /** Hears the head of the diff on show, or that none is ready: the checkout follows the diff, so the query
   * runs again on a newer one. */
  follow(head: string | undefined) {
    if (head === this.#head) return;
    this.#head = head;
    void this.#run();
  }

  /** Stops listening to the core, as the PR view goes. */
  stop() {
    this.#stopped = true;
    clearTimeout(this.#timer);
    void this.#listening?.then((stop) => {
      stop();
    });
  }

  /** The line under the box: the counts, the limit, the wait, or why nothing was found. */
  get summary(): string {
    const shown = this.#shown;
    switch (shown.kind) {
      case "empty":
        return "";
      case "waiting":
        return "Waiting for the checkout…";
      case "bad_query":
        return shown.message;
      case "problem":
        return `Tsugiki can't search: ${shown.message}`;
      case "no_checkout":
        return "There's no checkout of the PR to search. tsugiki.log may say why.";
      case "failed":
        return "Tsugiki couldn't read the PR's files. tsugiki.log says why.";
      case "results":
        return resultsLine(shown.files.length, shown.matches, shown.end);
    }
  }

  /** Whether the line under the box says why the query can't run or found nothing, rather than what it
   * found. */
  get troubled(): boolean {
    return !["empty", "waiting", "results"].includes(this.#shown.kind);
  }

  /** The rows of the results: each file, then each line of it unless it is folded. */
  readonly rows: readonly SearchRow[] = $derived.by(() => {
    const shown = this.#shown;
    if (shown.kind !== "results") return [];
    return shown.files.flatMap((file): SearchRow[] => {
      const folded = this.#folded.has(file.path);
      const matches = file.lines.reduce((sum, hit) => sum + hit.ranges.length, 0);
      const head: SearchRow = { kind: "file", file, matches, folded };
      return folded ? [head] : [head, ...file.lines.map((hit): SearchRow => ({ kind: "line", file, hit }))];
    });
  });

  /** The row the cursor is on, which ↑ and ↓ move: the first, until the user moves it, and when the row it
   * was on has gone. */
  get cursor(): number {
    const at = this.#cursor;
    if (at === undefined) return 0;
    return Math.max(
      0,
      this.rows.findIndex((row) => row.file.path === at.path && lineOf(row) === at.line),
    );
  }

  /** Puts the cursor on row `index`, held within the rows. */
  moveTo(index: number) {
    const row = this.rows[Math.min(Math.max(index, 0), this.rows.length - 1)];
    if (row !== undefined) this.#cursor = { path: row.file.path, line: lineOf(row) };
  }

  /** ←: folds the file under the cursor, or moves the cursor from a line to its file. */
  fold() {
    const row = this.rows[this.cursor];
    if (row?.kind === "line") this.#cursor = { path: row.file.path, line: undefined };
    else if (row !== undefined) this.#folded.add(row.file.path);
  }

  /** →: unfolds the file under the cursor, or moves the cursor from an open file to its first line. */
  unfold() {
    const row = this.rows[this.cursor];
    if (row?.kind !== "file") return;
    if (row.folded) this.#folded.delete(row.file.path);
    else this.moveTo(this.cursor + 1);
  }

  /** A click or Enter on file `path`'s row: folds the file, or unfolds it. */
  toggleFile(path: string) {
    if (this.#folded.has(path)) this.#folded.delete(path);
    else this.#folded.add(path);
  }

  /** Enter on the row under the cursor: the match on a line's row, or else the file's fold flips. */
  pick(): Match | undefined {
    const row = this.rows[this.cursor];
    if (row?.kind === "line") return { path: row.file.path, line: row.hit.line };
    if (row !== undefined) this.toggleFile(row.file.path);
    return undefined;
  }

  /** Runs the query in the box with the options on, in place of the one asked before, once the diff is ready
   * for the checkout to follow. An empty box shows nothing, and asks the core nothing. */
  async #run() {
    clearTimeout(this.#timer);
    this.#asked += 1;
    const asked = this.#asked;
    this.#id = undefined;
    if (this.text === "") {
      this.#shown = { kind: "empty" };
      return;
    }
    if (this.#head === undefined) {
      this.#shown = { kind: "waiting" };
      return;
    }
    const query: Query = {
      text: this.text,
      case: this.matchCase ? "matched" : "ignored",
      words: this.wholeWord ? "whole" : "within",
      syntax: this.regex ? "regex" : "literal",
    };
    // The listener is in place before the query goes, so nothing the core says of it is missed. Queries
    // that wait for it go in the order they were asked, so the core numbers the last one last.
    if (!(await this.#listen())) return;
    const answer = await commands.textSearch(this.#repository, query).catch(problem);
    if (asked !== this.#asked) return;
    if (answer.status === "error") {
      this.#shown = { kind: "problem", message: answer.error.message };
    } else if (answer.data.kind === "bad_query") {
      this.#shown = { kind: "bad_query", message: answer.data.message };
    } else {
      this.#id = answer.data.id;
      this.#folded.clear();
      this.#cursor = undefined;
      await this.#read();
    }
  }

  /** Listens to the core's word that what it found may have changed, once. False when Tsugiki can't hear
   * it, which the line under the box then says, and once the view has gone, which stops the listener. */
  async #listen(): Promise<boolean> {
    this.#listening ??= events.textSearchChanged.listen(() => void this.#read());
    try {
      await this.#listening;
      return !this.#stopped;
    } catch (e) {
      this.#listening = undefined;
      this.#shown = { kind: "problem", message: String(e) };
      return false;
    }
  }

  /** Asks the core what the latest query found. One read at a time: a change it hears during one reads
   * once more after it, so a burst of batches costs two reads and not one each. */
  async #read() {
    if (this.#id === undefined) return;
    if (this.#reading) {
      this.#again = true;
      return;
    }
    this.#reading = true;
    try {
      do {
        this.#again = false;
        this.#heard(await commands.textSearchState());
      } while (this.#again);
    } catch (e) {
      this.#shown = { kind: "problem", message: String(e) };
    } finally {
      this.#reading = false;
    }
  }

  /** Shows `state` if it answers the query asked last. */
  #heard(state: SearchState) {
    if (state.state === "idle" || state.id !== this.#id) return;
    if (state.state === "results") {
      this.#shown = { kind: "results", files: state.files, matches: state.matches, end: state.end };
    } else {
      this.#shown = { kind: state.state };
    }
  }
}

/** The line's number of a line's row, or nothing for a file's. */
const lineOf = (row: SearchRow): number | undefined => (row.kind === "line" ? row.hit.line : undefined);

/** A command's failure as the generated bindings type it. */
const problem = (e: unknown) => ({ status: "error" as const, error: { message: String(e) } });

/** What a query found so far, or in all, in `files` files. */
function resultsLine(files: number, matches: number, end: End): string {
  if (end === "stopped") return `Stopped at ${counted(matches, "result", "results")}`;
  if (matches === 0) return end === "running" ? "Searching…" : "No results";
  return `${counted(matches, "result", "results")} in ${counted(files, "file", "files")}`;
}
