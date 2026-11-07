// A PR's Files tab: its diff from the Repository's mirror, or why it isn't there yet. The rows stay in the
// core, and this asks for them in chunks near what the tab shows (ADR 0001), with the spans that highlight
// them (#178). A chunk's rows wait a moment for their spans, so a jump into a file the core has parsed shows
// it highlighted at once (#180).
import { SvelteMap } from "svelte/reactivity";
import {
  commands,
  events,
  type Cloning,
  type DiffFile_Serialize,
  type DiffRow_Serialize,
  type FileChange,
  type Highlight,
  type PrFiles_Serialize,
  type Span,
} from "$lib/data/bindings";
import { hud, type Jump, type JumpTimer } from "./hud.svelte";

export type PrFiles = PrFiles_Serialize;
export type DiffFile = DiffFile_Serialize;
export type DiffRow = DiffRow_Serialize;
export type { Cloning, FileChange, Highlight, Span };

/** Rows asked for at a time: about 25 screens, as the diff prototype found best. */
export const CHUNK = 512;

/** How long a chunk's rows wait for their spans before they show plain. The spans repaint the chunk when
 * they come. */
export const SPANS_WAIT = 50;

export class PrFilesState {
  /** Undefined until the core answers, and null if Tsugiki doesn't hold this PR. */
  view = $state<PrFiles | null>();
  error = $state<string>();
  /** The mirror's clone under way, which a Repository's first diff waits for. */
  cloning = $state<Cloning | null>(null);
  readonly #repository: string;
  readonly #number: number;
  readonly #jumps: JumpTimer;
  /** The rows the core has sent, by chunk, for the head of `view`. */
  #chunks = new SvelteMap<number, DiffRow[]>();
  /** The spans of each row, by chunk, which come after its rows. */
  #spans = new SvelteMap<number, Span[][]>();
  /** The chunks asked for. Nothing draws from it, and the effect that asks shouldn't rerun on it. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #asked = new Set<number>();
  #lastQuestion = 0;
  #stopListening: (() => void)[] = [];
  #stopped = false;

  constructor(repository: string, number: number, jumps: JumpTimer = hud) {
    this.#repository = repository;
    this.#number = number;
    this.#jumps = jumps;
  }

  /** Listens first, so it hears the head arrive or the clone end while it asks. */
  async load() {
    try {
      let heardCloning = false;
      const mine = (repository: string, items: number[]) =>
        repository === this.#repository && items.includes(this.#number);
      const listening = await Promise.all([
        events.headsFetched.listen(({ payload }) => {
          if (mine(payload.repository, payload.items)) void this.ask();
        }),
        // A sync that moves the PR's head changes its diff.
        events.changed.listen(({ payload }) => {
          if (mine(payload.repository, payload.items)) void this.ask();
        }),
        events.cloningChanged.listen(({ payload }) => {
          this.cloning = payload;
          heardCloning = true;
          if (payload === null) void this.ask();
        }),
      ]);
      this.#stopListening = listening;
      if (this.#stopped) {
        this.stop();
        return;
      }
      const cloning = await commands.cloning();
      // An event that arrived while asking is newer than this answer.
      if (!heardCloning) this.cloning = cloning;
      await this.ask();
    } catch (e) {
      this.error = String(e);
    }
  }

  stop() {
    this.#stopped = true;
    for (const stop of this.#stopListening) stop();
    this.#stopListening = [];
  }

  /** Asks the core what the tab shows now. A new head brings new rows. */
  async ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await this.#safely(() => commands.pullRequestFiles(this.#repository, this.#number));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion || answer === undefined) return;
    if (this.#head(answer) !== this.#head(this.view)) {
      this.#chunks.clear();
      this.#spans.clear();
      this.#asked.clear();
    }
    this.view = answer;
  }

  /** Row `index` of the diff, once the core has sent it. */
  row(index: number): DiffRow | undefined {
    return this.#chunks.get(Math.floor(index / CHUNK))?.[index % CHUNK];
  }

  /** The spans of row `index`, once the core has sent them. Until then the row shows plain. */
  spans(index: number): Span[] | undefined {
    return this.#spans.get(Math.floor(index / CHUNK))?.[index % CHUNK];
  }

  /** Asks for the chunks that hold rows `first` to `last`, and the one after, unless asked before. It lets
   * go of the chunks more than one away from those, so a long diff holds only the rows near what shows. A
   * chunk that holds some of those rows is a jump, which the HUD times. */
  need(first: number, last: number) {
    const view = this.view;
    if (view?.state !== "ready") return;
    const firstChunk = Math.floor(first / CHUNK);
    const shownChunk = Math.floor(last / CHUNK);
    const lastChunk = Math.min(shownChunk + 1, Math.ceil(view.rows / CHUNK) - 1);
    for (const chunk of this.#asked) {
      if (chunk >= firstChunk - 1 && chunk <= lastChunk + 1) continue;
      this.#asked.delete(chunk);
      this.#chunks.delete(chunk);
      this.#spans.delete(chunk);
    }
    // The chunks that hold rows that show, which the chunk after waits for, so that the core parses their
    // files first.
    const shown: Promise<void>[] = [];
    for (let chunk = firstChunk; chunk <= lastChunk; chunk++) {
      if (this.#asked.has(chunk)) continue;
      this.#asked.add(chunk);
      if (chunk > shownChunk) {
        void Promise.all(shown).then(() => this.#fetch(view.head, chunk, undefined));
        continue;
      }
      // One jump for the rows that show, timed by the first of their chunks.
      const jump: Jump | undefined = shown.length === 0 ? this.#jumps.jumped() : undefined;
      shown.push(this.#fetch(view.head, chunk, jump));
    }
  }

  /** Asks for a chunk's rows and spans together. The rows show once the spans come, or once they have
   * waited `SPANS_WAIT` since the ask, whichever is first. */
  async #fetch(head: string, chunk: number, jump: Jump | undefined) {
    const spans = this.#fetchSpans(head, chunk);
    const waited = new Promise<undefined>((resolve) => setTimeout(resolve, SPANS_WAIT));
    const rows = await this.#safely(() => commands.pullRequestRows(this.#chunk(head, chunk)));
    if (rows === undefined) {
      this.#asked.delete(chunk);
      return;
    }
    const early = await Promise.race([spans, waited]);
    if (!this.#wanted(head, chunk)) return;
    if (early !== undefined) this.#spans.set(chunk, early);
    this.#chunks.set(chunk, rows);
    jump?.rowsShown(early !== undefined);
    if (early !== undefined) return;
    const late = await spans;
    if (late === undefined || !this.#wanted(head, chunk)) return;
    this.#spans.set(chunk, late);
    jump?.spansShown();
  }

  /** A chunk's spans, or nothing if they don't come. Its rows fail for the same causes, and say so. */
  async #fetchSpans(head: string, chunk: number): Promise<Span[][] | undefined> {
    const answer = await commands.pullRequestSpans(this.#chunk(head, chunk)).catch(() => undefined);
    return answer?.status === "ok" ? answer.data : undefined;
  }

  #chunk(head: string, chunk: number) {
    return {
      repository: this.#repository,
      number: this.#number,
      head,
      start: chunk * CHUNK,
      end: (chunk + 1) * CHUNK,
    };
  }

  /** Whether what came for `chunk` at `head` still belongs: what came for an older head would land among the
   * newer head's, and a chunk let go of is far from what shows. */
  #wanted(head: string, chunk: number): boolean {
    return this.#head(this.view) === head && this.#asked.has(chunk);
  }

  #head(view: PrFiles | null | undefined): string | undefined {
    return view?.state === "ready" ? view.head : undefined;
  }

  /** What a command answered, or undefined after saying why it failed. */
  async #safely<T>(
    ask: () => Promise<{ status: "ok"; data: T } | { status: "error"; error: { message: string } }>,
  ): Promise<T | undefined> {
    const answer = await ask().catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    if (answer.status === "error") {
      this.error = answer.error.message;
      return undefined;
    }
    this.error = undefined;
    return answer.data;
  }
}
