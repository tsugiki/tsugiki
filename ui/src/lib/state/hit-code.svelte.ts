// The code the Definition peek shows (#439): the hit's file at the hit's commit, or from disk for a file
// outside the Repository. The core reads it a chunk of lines at a time near the rows that show, so a big file
// such as CPython's `Objects/unicodeobject.c` opens without the window waiting for all of it. Each chunk's
// spans come apart from its lines, as the diff's do (#455): the lines wait a moment for them, then show plain
// until the core has parsed the file, and the spans paint them when they come.

import { SvelteMap } from "svelte/reactivity";
import { commands, type CodeFile, type DefinitionCode, type Hit, type Span } from "$lib/data/bindings";
import { wordAt, type Picked } from "./pick";
import { waitForSpans } from "./spans-wait";

/** How many lines the window asks the core for at a time. */
export const CODE_CHUNK = 200;

/** The file `hit` is in, as the core reads it. */
export function codeFile(hit: Hit): CodeFile {
  const place = hit.place;
  return place.kind === "repository"
    ? { kind: "repository", path: place.path, commit: place.commit }
    : { kind: "outside", path: place.path };
}

/** The word the user picked in a hit's code (#447): its line, counted from 1, and its place in that line. */
export type CodePick = Picked & { line: number; word: string };

/** The lines of one hit's file that the window has read, and asks for more of as the peek scrolls. */
export class HitCode {
  /** How many lines the file has, once the first chunk has come. */
  count = $state<number>();
  /** Why the core couldn't read the file. */
  problem = $state<string>();
  /** How many columns the widest line read takes, a tab as 8, so the peek scrolls far enough sideways. */
  widest = $state(0);
  /** Where the user scrolled the code to, which the peek goes back to when the diff draws it again after
   * it scrolled out of sight. Until then the peek scrolls to the hit, and so does the file view once Go to
   * definition lands on a line of it (#457). */
  scrollTop: number | undefined;
  /** The word the user picked in the code, which `Alt+F12` looks up from inside the peek (#447), and `F12`
   * and `Alt+F12` from the file view, which shows this code once Go to definition opens it (#457). A crumb
   * keeps the code, so a peek it brings back shows the word that was followed. Picking another hit makes new
   * code, with no pick. */
  pick = $state.raw<CodePick>();
  #chunks = new SvelteMap<number, readonly string[]>();
  /** The spans of each chunk's lines, which may come after its lines. */
  #spans = new SvelteMap<number, Span[][]>();
  /** The chunks asked for. Nothing draws from it, and the effect that asks shouldn't rerun on it. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #asked = new Set<number>();
  /** This hit's place among the lookup's hits, counted from 0. The peek shows the hit whose code it holds. */
  readonly index: number;
  readonly #repository: string;
  readonly #file: CodeFile;

  constructor(repository: string, hit: Hit, index: number) {
    this.index = index;
    this.#repository = repository;
    this.#file = codeFile(hit);
  }

  /** Line `line`'s text, counted from 1, once its chunk has come. */
  line(line: number): string | undefined {
    const index = line - 1;
    return this.#chunks.get(Math.floor(index / CODE_CHUNK))?.[index % CODE_CHUNK];
  }

  /** Line `line`'s spans, counted from 1, once they have come. Until then the line shows plain. */
  spans(line: number): Span[] | undefined {
    const index = line - 1;
    return this.#spans.get(Math.floor(index / CODE_CHUNK))?.[index % CODE_CHUNK];
  }

  /** Picks the word at offset `offset` of line `line`, selected for a double-click, as the diff picks one
   * (#437), and returns the pick. With no word there, or a line not read yet, it drops the pick. */
  pickWord(line: number, offset: number, selected: boolean): CodePick | undefined {
    const text = this.line(line) ?? "";
    const word = wordAt(text, offset);
    this.pick = word && {
      line,
      word: text.slice(word.start, word.end),
      start: word.start,
      end: word.end,
      caret: selected ? null : offset,
    };
    return this.pick;
  }

  /** Asks for the chunks that hold lines `first` to `last`, counted from 1, unless asked before. */
  show(first: number, last: number) {
    const end = this.count === undefined ? last : Math.min(last, this.count);
    for (
      let chunk = Math.floor((first - 1) / CODE_CHUNK);
      chunk <= Math.floor((end - 1) / CODE_CHUNK);
      chunk++
    ) {
      if (chunk < 0 || this.#asked.has(chunk)) continue;
      this.#asked.add(chunk);
      void this.#read(chunk);
    }
  }

  /** Forgets why the file couldn't be read, so the next `show` asks again, as when the history brings a
   * file view back (#456). A file that has shown lines, or still waits for its first, keeps what it has. */
  retry() {
    if (this.problem === undefined) return;
    this.problem = undefined;
    this.#asked.clear();
  }

  /** Asks for a chunk's lines and spans together. The lines show once the spans come, or once they have
   * waited `SPANS_WAIT` since the ask, whichever is first. */
  async #read(chunk: number) {
    const start = chunk * CODE_CHUNK + 1;
    const code = { repository: this.#repository, file: this.#file, start, end: start + CODE_CHUNK };
    const spans = readSpans(code);
    const waited = waitForSpans(spans);
    const read = await commands.definitionCode(code).catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    if (read.status === "error") {
      // A file that showed no line says why. One that did keeps its lines, and the next scroll asks again.
      if (this.count === undefined) this.problem = read.error.message;
      else this.#asked.delete(chunk);
      return;
    }
    const early = await waited();
    if (early !== undefined) this.#spans.set(chunk, early);
    this.count = read.data.count;
    const widest = Math.max(
      0,
      ...read.data.lines.map((line) => line.length + 7 * (line.split("\t").length - 1)),
    );
    this.widest = Math.max(this.widest, widest);
    this.#chunks.set(chunk, read.data.lines);
    if (early !== undefined) return;
    const late = await spans;
    if (late !== undefined) this.#spans.set(chunk, late);
  }
}

/** The spans of the lines `code` names, or nothing if they don't come: the lines then stay plain, and say
 * why if they fail too. */
async function readSpans(code: DefinitionCode): Promise<Span[][] | undefined> {
  const answer = await commands.definitionSpans(code).catch(() => undefined);
  return answer?.status === "ok" ? answer.data : undefined;
}
