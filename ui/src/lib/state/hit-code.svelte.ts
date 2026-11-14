// The code the Definition peek shows (#439): the hit's file at the hit's commit, or from disk for a file
// outside the Repository. The core reads it a chunk of lines at a time near the rows that show, so a big file
// such as CPython's `Objects/unicodeobject.c` opens without the window waiting for all of it.

import { SvelteMap } from "svelte/reactivity";
import { commands, type CodeFile, type Hit } from "$lib/data/bindings";

/** How many lines the window asks the core for at a time. */
export const CODE_CHUNK = 200;

/** The file `hit` is in, as the core reads it. */
export function codeFile(hit: Hit): CodeFile {
  const place = hit.place;
  return place.kind === "repository"
    ? { kind: "repository", path: place.path, commit: place.commit }
    : { kind: "outside", path: place.path };
}

/** The lines of one hit's file that the window has read, and asks for more of as the peek scrolls. */
export class HitCode {
  /** How many lines the file has, once the first chunk has come. */
  count = $state<number>();
  /** Why the core couldn't read the file. */
  problem = $state<string>();
  /** How many columns the widest line read takes, a tab as 8, so the peek scrolls far enough sideways. */
  widest = $state(0);
  /** Where the user scrolled the code to, which the peek goes back to when the diff draws it again after
   * it scrolled out of sight. Until then the peek scrolls to the hit. */
  scrollTop: number | undefined;
  #chunks = new SvelteMap<number, readonly string[]>();
  /** The chunks asked for. Nothing draws from it, and the effect that asks shouldn't rerun on it. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #asked = new Set<number>();
  readonly #repository: string;
  readonly #file: CodeFile;

  constructor(repository: string, hit: Hit) {
    this.#repository = repository;
    this.#file = codeFile(hit);
  }

  /** Line `line`'s text, counted from 1, once its chunk has come. */
  line(line: number): string | undefined {
    const index = line - 1;
    return this.#chunks.get(Math.floor(index / CODE_CHUNK))?.[index % CODE_CHUNK];
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

  async #read(chunk: number) {
    const start = chunk * CODE_CHUNK + 1;
    const code = { repository: this.#repository, file: this.#file, start, end: start + CODE_CHUNK };
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
    this.count = read.data.count;
    const widest = Math.max(
      0,
      ...read.data.lines.map((line) => line.length + 7 * (line.split("\t").length - 1)),
    );
    this.widest = Math.max(this.widest, widest);
    this.#chunks.set(chunk, read.data.lines);
  }
}
