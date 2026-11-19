// The rail's Search tab (#686): a query over the code of the PR on show, at the ends of the diff its Files
// tab shows, as the core's text search runs it in the Lookup checkout (#685). Each query names those ends,
// so a query waits until the diff is ready, and runs again on a newer one. Typing runs the query once the
// keys pause, and each toggle runs it at once. The core numbers each query and says when what it found may
// have changed, and this shows only what answers the query it asked last, and cancels a query nobody wants
// any more. The results group by file, and the keys move a cursor through the rows, fold and unfold a file,
// and open a match.
import { SvelteSet } from "svelte/reactivity";
import {
  commands,
  events,
  type End,
  type FileHits,
  type LineHit,
  type Query,
  type SearchId,
  type SearchState,
} from "$lib/data/bindings";
import { ask } from "./answer";
import { counted } from "./counted";
import { type DiffEnds, diffKey } from "./diff-ends";
import type { PrFilesState } from "./pr-files.svelte";

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

/** A match to open: the file's path and the line's number at the head of the diff the query searched. */
export type Match = { path: string; line: number; ends: DiffEnds };

/** What the search can run on, from the Files tab: the diff's ends once it is ready, or why there are none
 * yet. `loading` and `offline` come on their own, `asking` once the user answers the clone's question in
 * the Files tab, and `absent` and `failed` never on their own. */
export type SearchDiff =
  | ({ kind: "ready" } & DiffEnds)
  | { kind: "loading" }
  | { kind: "offline" }
  | { kind: "asking" }
  | { kind: "absent" }
  | { kind: "failed" };

/** What the Files tab, `files`, gives the search to run on: its diff, or why it has none, as the tab says
 * it. PR commits that wait for the network wait, as the tab tries again by itself, and so does a fetch
 * the network explains. A failed read, or a fetch that stopped for another reason, has failed.
 *
 * A failed fetch's word that comes after Try again, from the fetch before it, says the diff failed while
 * the new fetch runs: the tab can't tell it from the new fetch's own, and nor can this. */
export function searchDiff(
  files: Pick<PrFilesState, "view" | "error" | "unfetched" | "waitsForNetwork">,
): SearchDiff {
  const view = files.view;
  if (view?.state === "ready") return { kind: "ready", base: view.base, head: view.head };
  if (view === null) return { kind: "absent" };
  const fetching = view?.state === "fetching" || view?.state === "finding_head";
  if (fetching && files.waitsForNetwork) return { kind: "offline" };
  const failed = files.error !== undefined || files.unfetched !== undefined || view?.state === "unread";
  if (failed) return { kind: "failed" };
  return view?.state === "asking" ? { kind: "asking" } : { kind: "loading" };
}

/** What the tab shows under the box. */
type Shown =
  | { kind: "empty" }
  | { kind: "unready"; why: Exclude<SearchDiff["kind"], "ready"> }
  | { kind: "waiting" }
  | { kind: "bad_query"; message: string }
  | { kind: "problem"; message: string }
  | { kind: "no_checkout" }
  | { kind: "failed" }
  | { kind: "results"; files: FileHits[]; matches: number; end: End; ends: DiffEnds };

/** A row by what it shows, so the cursor stays on it as results come in above it. */
type RowKey = { path: string; line: number | undefined };

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
  /** How many queries the core has taken, so the list goes back to its top for each one's results. */
  answered = $state(0);
  /** Whether the box has the focus, when the selection is the box's own text. */
  boxFocused = false;
  #shown = $state.raw<Shown>({ kind: "empty" });
  #folded = new SvelteSet<string>();
  #cursorKey = $state.raw<RowKey>();
  readonly #repository: string;
  /** What the search can run on, which the Files tab's diff gives. */
  #diff: SearchDiff = { kind: "loading" };
  /** The core's number for the query asked last, once it has answered, with the ends it searches. */
  #asking: { id: SearchId; ends: DiffEnds } | undefined;
  /** How many queries this has asked, so a late answer to an older one changes nothing. */
  #asked = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #listening: Promise<() => void> | undefined;
  #reading = false;
  /** The core said something changed while a read was under way, which may have missed it. */
  #again = false;
  #stopped = false;
  /** Reads the PR's diff again, which moves the Lookup checkout back to it. */
  readonly #reread: () => Promise<void>;
  /** Whether the query on show has read the diff again once already, after the core found no checkout of
   * it. */
  #rereadDone = false;

  /** A search in `repository`'s PR, which calls `reread` to read the PR's diff again when the core finds no
   * checkout of it: another PR's read may have moved the checkout away. */
  constructor(repository: string, reread: () => Promise<void> = () => Promise.resolve()) {
    this.#repository = repository;
    this.#reread = reread;
  }

  /** The box's text as the user types it: the query runs once the keys pause. */
  type(text: string) {
    this.text = text;
    this.#rereadDone = false;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.#run(), SEARCH_PAUSE);
  }

  /** Turns `option` on or off, and runs the query at once. */
  toggle(option: SearchOption) {
    if (option === "case") this.matchCase = !this.matchCase;
    else if (option === "words") this.wholeWord = !this.wholeWord;
    else this.regex = !this.regex;
    this.#rereadDone = false;
    void this.#run();
  }

  /** Whether the box holds `text` as `seed` would put it there. */
  holds(text: string): boolean {
    return this.text === (this.regex ? literalRegex(text) : text);
  }

  /** Puts `text`, the picked word or the selected text, in the box, as a regex matches it with regex on, and
   * runs the query at once. */
  seed(text: string) {
    this.text = this.regex ? literalRegex(text) : text;
    this.#rereadDone = false;
    void this.#run();
  }

  /** Hears what the search can run on, `diff`: the query runs again on a diff with other ends, and says why
   * it waits while there is none. */
  follow(diff: SearchDiff) {
    if (keyOf(diff) === keyOf(this.#diff)) return;
    this.#diff = diff;
    this.#rereadDone = false;
    void this.#run();
  }

  /** Stops listening to the core, and cancels the query running, as the PR view goes. */
  stop() {
    this.#stopped = true;
    clearTimeout(this.#timer);
    // An answer on its way is then to an older query, and cancels itself.
    this.#asked += 1;
    this.#cancel(this.#asking?.id);
    this.#asking = undefined;
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
      case "unready":
        return UNREADY[shown.why];
      case "waiting":
        return UNREADY.loading;
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
   * found or what it waits for. */
  get troubled(): boolean {
    const shown = this.#shown;
    if (shown.kind === "unready") return shown.why === "absent" || shown.why === "failed";
    return !["empty", "waiting", "results"].includes(shown.kind);
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
   * was on has gone. Each row drawn reads it, so it is found once for the rows and the cursor. */
  readonly cursor: number = $derived.by(() => {
    const at = this.#cursorKey;
    if (at === undefined) return 0;
    return Math.max(
      0,
      this.rows.findIndex((row) => row.file.path === at.path && lineOf(row) === at.line),
    );
  });

  /** Puts the cursor on row `index`, held within the rows. */
  moveTo(index: number) {
    const row = this.rows[Math.min(Math.max(index, 0), this.rows.length - 1)];
    if (row !== undefined) this.#cursorKey = { path: row.file.path, line: lineOf(row) };
  }

  /** ←: folds the file under the cursor, or moves the cursor from a line to its file. */
  fold() {
    const row = this.rows[this.cursor];
    if (row?.kind === "line") this.#cursorKey = { path: row.file.path, line: undefined };
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
    const shown = this.#shown;
    const row = this.rows[this.cursor];
    if (row?.kind === "line" && shown.kind === "results")
      return { path: row.file.path, line: row.hit.line, ends: shown.ends };
    if (row !== undefined) this.toggleFile(row.file.path);
    return undefined;
  }

  /** Runs the query in the box with the options on, in place of the one asked before, once the diff is ready
   * to name the ends it searches. An empty box shows nothing, and asks the core nothing. A query nobody
   * wants any more, as the box empties or the diff goes, is cancelled, and so is one a bad query leaves
   * running. */
  async #run() {
    clearTimeout(this.#timer);
    this.#asked += 1;
    const asked = this.#asked;
    const before = this.#asking?.id;
    this.#asking = undefined;
    const diff = this.#diff;
    if (this.text === "") {
      this.#shown = { kind: "empty" };
      this.#cancel(before);
      return;
    }
    if (diff.kind !== "ready") {
      this.#shown = { kind: "unready", why: diff.kind };
      this.#cancel(before);
      return;
    }
    const query: Query = {
      text: this.text,
      case: this.matchCase ? "matched" : "ignored",
      words: this.wholeWord ? "whole" : "within",
      syntax: this.regex ? "regex" : "literal",
    };
    const ends = { base: diff.base, head: diff.head };
    // The listener is in place before the query goes, so nothing the core says of it is missed. A newer
    // query asked meanwhile goes in place of this one, which then asks nothing. Whatever stops it here
    // cancels the query before it, as nothing else knows of that one any more.
    if (!(await this.#listen()) || asked !== this.#asked) {
      this.#cancel(before);
      return;
    }
    const answer = await ask(commands.textSearch({ repository: this.#repository, ...ends, query }));
    const running = answer.status === "ok" && answer.data.kind === "running" ? answer.data.id : undefined;
    if (asked !== this.#asked) {
      this.#cancel(running);
      return;
    }
    if (answer.status === "error") {
      this.#shown = { kind: "problem", message: answer.error.message };
      this.#cancel(before);
    } else if (answer.data.kind === "bad_query") {
      this.#shown = { kind: "bad_query", message: answer.data.message };
      this.#cancel(before);
    } else {
      this.#asking = { id: answer.data.id, ends };
      this.answered += 1;
      this.#folded.clear();
      this.#cursorKey = undefined;
      await this.#read();
    }
  }

  /** Asks the core to cancel query `id`, if there is one, which it does only while that query runs. */
  #cancel(id: SearchId | undefined) {
    if (id !== undefined) void commands.textSearchCancel(id).catch(() => undefined);
  }

  /** Listens to the core's word that what it found may have changed, once. False when Tsugiki can't hear
   * it, which the line under the box then says, and once the view has gone, which places no listener and
   * stops one on its way. */
  async #listen(): Promise<boolean> {
    if (this.#stopped) return false;
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
    if (this.#asking === undefined) return;
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

  /** Shows `state` if it answers the query asked last. The first time the core finds no checkout of the
   * query's diff, this reads the diff again, which moves the checkout back to it, and runs the query once
   * more. */
  #heard(state: SearchState) {
    const asking = this.#asking;
    if (state.state === "idle" || state.id !== asking?.id) return;
    if (state.state === "no_checkout" && !this.#rereadDone) {
      this.#rereadDone = true;
      this.#shown = { kind: "waiting" };
      const asked = this.#asked;
      void this.#reread().then(() => {
        if (asked === this.#asked) void this.#run();
      });
    } else if (state.state === "results") {
      const { files, matches, end } = state;
      this.#shown = { kind: "results", files, matches, end, ends: asking.ends };
    } else {
      this.#shown = { kind: state.state };
    }
  }
}

/** What the line under the box says while the search has no diff to run on. */
const UNREADY: Record<Exclude<SearchDiff["kind"], "ready">, string> = {
  loading: "Waiting for the checkout…",
  offline: "The search waits for the PR's commits, which come once GitHub answers.",
  asking: "The search waits for the Repository's clone, which waits for an answer in the Files tab.",
  absent: "Tsugiki doesn't hold this PR, so there's nothing to search.",
  failed: "The PR's diff didn't load, so there's nothing to search. The Files tab says why.",
};

/** What tells what the search runs on from anything else: the diff's key, or why there is no diff. */
const keyOf = (diff: SearchDiff): string => (diff.kind === "ready" ? diffKey(diff) : diff.kind);

/** The line's number of a line's row, or nothing for a file's. */
const lineOf = (row: SearchRow): number | undefined => (row.kind === "line" ? row.hit.line : undefined);

/** What a query found so far, or in all, in `files` files. */
function resultsLine(files: number, matches: number, end: End): string {
  if (end === "stopped") return `Stopped at ${counted(matches, "result", "results")}`;
  if (matches === 0) return end === "running" ? "Searching…" : "No results";
  return `${counted(matches, "result", "results")} in ${counted(files, "file", "files")}`;
}
