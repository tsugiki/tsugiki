// A PR's Files tab: its diff from the Repository's mirror, or why it isn't there yet. The rows stay in the
// core, and this asks for them in chunks near what the tab shows (ADR 0001), with the spans that highlight
// them (#178). A chunk's rows wait a moment for their spans, so a jump into a file the core has parsed shows
// it highlighted at once (#180). Generated files start collapsed, and their rows aren't asked for until the
// user opens one (#181). The user can expand the lines a gap between hunks leaves out, which the core reads
// from the file at the PR's head (#182).
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import {
  commands,
  events,
  type Cloning,
  type DiffFile_Serialize,
  type DiffRow_Serialize,
  type FileChange,
  type Gap,
  type HeadLine,
  type Highlight,
  type LinkTo,
  type Rest,
  type NewsBlock,
  type NewsEntry_Serialize,
  type NewsPiece,
  type PrFiles_Serialize,
  type Span,
} from "$lib/data/bindings";
import { DiffLayout, type Shown } from "./diff-layout";
import { hud, type Jump, type JumpTimer } from "./hud.svelte";

export type PrFiles = PrFiles_Serialize;
export type DiffFile = DiffFile_Serialize;
export type DiffRow = DiffRow_Serialize;
export type NewsEntry = NewsEntry_Serialize;
/** A diff the tab shows, from its merge base to its head. */
type Ready = Extract<PrFiles, { state: "ready" }>;
export type { Cloning, FileChange, Gap, HeadLine, Highlight, LinkTo, NewsBlock, NewsPiece, Shown, Span };

/** Rows asked for at a time: about 25 screens, as the diff prototype found best. */
export const CHUNK = 512;

/** Lines a click shows above or below a gap, as on GitHub. */
export const EXPAND = 20;

/** The line past any file's last, to ask for a file to its end. */
const END = 2 ** 32 - 1;

/** How long a chunk's rows wait for their spans before they show plain. The spans repaint the chunk when
 * they come. */
export const SPANS_WAIT = 50;

export class PrFilesState {
  /** Undefined until the core answers, and null if Tsugiki doesn't hold this PR. */
  view = $state<PrFiles | null>();
  error = $state<string>();
  /** The mirror's clone under way, which a Repository's first diff waits for. */
  cloning = $state<Cloning | null>(null);
  /** The rows that show, and where: a Generated file the user hasn't opened shows only its header, and a
   * gap the user expanded shows the lines it left out. */
  layout = $derived.by(() => {
    const view = this.view;
    const ready = view?.state === "ready";
    return new DiffLayout(ready ? view.rows : 0, ready ? view.files : [], (file) => this.collapsed(file), {
      gap: (file, gap) => this.#shown(file, gap),
      rest: (file, rest) => this.#after(file, rest),
    });
  });
  /** How many characters the longest expanded line has, so the tab makes room for it as for the diff's. */
  columns = $state(0);
  readonly #repository: string;
  readonly #number: number;
  readonly #jumps: JumpTimer;
  /** The rows the core has sent, by chunk, for the diff `view` shows. */
  #chunks = new SvelteMap<number, DiffRow[]>();
  /** The spans of each row, by chunk, which come after its rows. */
  #spans = new SvelteMap<number, Span[][]>();
  /** The Generated files the user has opened, by their place among the files. */
  #opened = new SvelteSet<number>();
  /** The lines expanded gaps show, by `file:line` at the head, once the core sends them. A gap shows the
   * lines held from each of its ends, so a click that fails shows nothing, and two that overlap ask for
   * the same lines. */
  #lines = new SvelteMap<string, HeadLine>();
  /** How many lines each file has at the head, by its place, once an answer stopped short at its end. */
  #lengths = new SvelteMap<number, number>();
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

  /** Asks the core what the tab shows now. A new head brings new rows, as does a new merge base: a PR
   * moved to another Target branch keeps its head, and its diff can list other files in the places the tab
   * holds. */
  async ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await this.#safely(() => commands.pullRequestFiles(this.#repository, this.#number));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion || answer === undefined) return;
    if (this.#diff(answer) !== this.#diff(this.view)) {
      this.#chunks.clear();
      this.#spans.clear();
      this.#asked.clear();
      this.#opened.clear();
      this.#lines.clear();
      this.#lengths.clear();
      this.columns = 0;
    }
    this.view = answer;
  }

  /** Whether the file at `file` among the diff's files shows only its header: a Generated file starts so. */
  collapsed(file: number): boolean {
    const view = this.view;
    return view?.state === "ready" && view.files[file]?.generated === true && !this.#opened.has(file);
  }

  /** Opens a collapsed Generated file, or collapses it again. The core gave the chunks the tab holds no
   * spans for the file's rows while it was collapsed, so opening it asks for their spans again. The tab
   * holds a few chunks, and the core keeps the trees of the other files, so that costs little. */
  toggle(file: number) {
    if (this.#opened.has(file)) {
      this.#opened.delete(file);
      return;
    }
    this.#opened.add(file);
    const view = this.view;
    if (view?.state !== "ready") return;
    for (const chunk of this.#asked) void this.#respan(view, chunk);
  }

  /** Asks for a chunk's spans again, and paints them if they come while it still belongs. */
  async #respan(diff: Ready, chunk: number) {
    const spans = await this.#fetchSpans(diff, chunk);
    if (spans !== undefined && this.#wanted(diff, chunk)) this.#spans.set(chunk, spans);
  }

  /** Line `line` at the head of file `file`, which an expanded gap shows. The layout shows only lines the
   * core has sent, so one it hasn't is blank. */
  line(file: number, line: number): HeadLine {
    return this.#lines.get(`${String(file)}:${String(line)}`) ?? { text: "", spans: [] };
  }

  /** How many of `gap`'s lines show from its top and from its bottom: those the core has sent, which
   * follow on from each end. */
  #shown(file: number, gap: Gap): { top: number; bottom: number } {
    const top = this.#run(file, gap.new, 1, gap.lines);
    const bottom = this.#run(file, gap.new + gap.lines - 1, -1, gap.lines - top);
    return { top, bottom };
  }

  /** How many lines of file `file` the tab holds one after another from `line` on, going by `step`, up to
   * `most`. */
  #run(file: number, line: number, step: 1 | -1, most: number): number {
    let held = 0;
    while (held < most && this.#lines.has(`${String(file)}:${String(line + step * held)}`)) held += 1;
    return held;
  }

  /** The lines that show after file `file`'s last hunk, from `rest` on, and whether the file ends there. */
  #after(file: number, rest: Rest): { lines: number; ended: boolean } {
    const length = this.#lengths.get(file);
    const lines = this.#run(file, rest.new, 1, length === undefined ? Infinity : length - rest.new + 1);
    return { lines, ended: length !== undefined && rest.new + lines > length };
  }

  /** Shows `EXPAND` more lines of `gap` in file `file`, from its bottom up, above its hunk. */
  expandAbove(file: number, gap: Gap) {
    const { top, bottom } = this.#shown(file, gap);
    const end = gap.new + gap.lines;
    void this.#fetchLines(file, end - Math.min(bottom + EXPAND, gap.lines - top), end - bottom);
  }

  /** Shows `EXPAND` more lines of `gap` in file `file`, from its top down, below the hunk before. */
  expandBelow(file: number, gap: Gap) {
    const { top, bottom } = this.#shown(file, gap);
    void this.#fetchLines(file, gap.new + top, gap.new + Math.min(top + EXPAND, gap.lines - bottom));
  }

  /** Shows every line `gap` in file `file` still hides. */
  expandGap(file: number, gap: Gap) {
    const { top, bottom } = this.#shown(file, gap);
    void this.#fetchLines(file, gap.new + top, gap.new + gap.lines - bottom);
  }

  /** Shows `EXPAND` more lines after file `file`'s last hunk. Fewer than asked for end the file, and the
   * button goes. */
  async expandRest(file: number) {
    const view = this.view;
    const rest = view?.state === "ready" ? view.files[file]?.rest : undefined;
    if (rest === undefined || rest === null) return;
    const start = rest.new + this.#after(file, rest).lines;
    await this.#fetchLines(file, start, start + EXPAND);
  }

  /** Shows the whole of file `file`: every gap, and the lines after its last hunk. */
  async expandFile(file: number) {
    await this.#fetchLines(file, 1, END);
  }

  /** Asks the core for lines `start` up to `end` of file `file` at the head, and keeps those that come
   * while the tab still shows the same diff. Fewer than asked for say where the file ends. */
  async #fetchLines(file: number, start: number, end: number) {
    const view = this.view;
    if (view?.state !== "ready" || start >= end) return;
    const lines = await this.#safely(() =>
      commands.pullRequestLines({
        repository: this.#repository,
        number: this.#number,
        head: view.head,
        base: view.base,
        file,
        start,
        end,
      }),
    );
    if (lines === undefined || this.#diff(this.view) !== this.#diff(view)) return;
    lines.forEach((line, at) => this.#lines.set(`${String(file)}:${String(start + at)}`, line));
    this.columns = lines.reduce((widest, line) => Math.max(widest, [...line.text].length), this.columns);
    if (start + lines.length < end) this.#lengths.set(file, start + lines.length - 1);
  }

  /** Row `index` of the diff, once the core has sent it. */
  row(index: number): DiffRow | undefined {
    return this.#chunks.get(Math.floor(index / CHUNK))?.[index % CHUNK];
  }

  /** The spans of row `index`, once the core has sent them. Until then the row shows plain. */
  spans(index: number): Span[] | undefined {
    return this.#spans.get(Math.floor(index / CHUNK))?.[index % CHUNK];
  }

  /** Asks for the chunks that hold the rows that show from `first` to `last`, and those that hold the next
   * `CHUNK` rows that show, unless asked before. It lets go of the chunks further away, so a long diff holds
   * only the rows near what shows. A chunk that holds some of those rows is a jump, which the HUD times.
   * Rows a collapsed file hides aren't asked for. */
  need(first: number, last: number) {
    const view = this.view;
    if (view?.state !== "ready") return;
    const layout = this.layout;
    const shownChunks = chunksOf(layout.pieces(first, last));
    const after = chunksOf(layout.pieces(last + 1, last + CHUNK)).filter(
      (chunk) => !shownChunks.includes(chunk),
    );
    const near = chunksOf(layout.pieces(first - CHUNK, last + 2 * CHUNK));
    for (const chunk of this.#asked) {
      if (near.includes(chunk)) continue;
      this.#asked.delete(chunk);
      this.#chunks.delete(chunk);
      this.#spans.delete(chunk);
    }
    // The chunks that hold rows that show, which the chunks after wait for, so that the core parses their
    // files first.
    const shown: Promise<void>[] = [];
    for (const chunk of shownChunks) {
      if (this.#asked.has(chunk)) continue;
      this.#asked.add(chunk);
      // One jump for the rows that show, timed by the first of their chunks.
      const jump: Jump | undefined = shown.length === 0 ? this.#jumps.jumped() : undefined;
      shown.push(this.#fetch(view, chunk, jump));
    }
    for (const chunk of after) {
      if (this.#asked.has(chunk)) continue;
      this.#asked.add(chunk);
      void Promise.all(shown).then(() => this.#fetch(view, chunk, undefined));
    }
  }

  /** Asks for a chunk's rows and spans together. The rows show once the spans come, or once they have
   * waited `SPANS_WAIT` since the ask, whichever is first. */
  async #fetch(diff: Ready, chunk: number, jump: Jump | undefined) {
    const spans = this.#fetchSpans(diff, chunk);
    const waited = new Promise<undefined>((resolve) => setTimeout(resolve, SPANS_WAIT));
    const rows = await this.#safely(() => commands.pullRequestRows(this.#chunk(diff, chunk)));
    if (rows === undefined) {
      // A chunk of an older diff that fails leaves the newer diff's ask for the same chunk alone.
      if (this.#wanted(diff, chunk)) this.#asked.delete(chunk);
      return;
    }
    const early = await Promise.race([spans, waited]);
    if (!this.#wanted(diff, chunk)) return;
    if (early !== undefined) this.#spans.set(chunk, early);
    this.#chunks.set(chunk, rows);
    jump?.rowsShown(early !== undefined);
    if (early !== undefined) return;
    const late = await spans;
    if (late === undefined || !this.#wanted(diff, chunk)) return;
    this.#spans.set(chunk, late);
    jump?.spansShown();
  }

  /** A chunk's spans, or nothing if they don't come. Its rows fail for the same causes, and say so. */
  async #fetchSpans(diff: Ready, chunk: number): Promise<Span[][] | undefined> {
    const answer = await commands
      .pullRequestSpans(this.#chunk(diff, chunk), [...this.#opened])
      .catch(() => undefined);
    return answer?.status === "ok" ? answer.data : undefined;
  }

  /** A chunk of `diff`, which names the commits it runs between so the core reads it from those. */
  #chunk(diff: Ready, chunk: number) {
    return {
      repository: this.#repository,
      number: this.#number,
      head: diff.head,
      base: diff.base,
      start: chunk * CHUNK,
      end: (chunk + 1) * CHUNK,
    };
  }

  /** Whether what came for `chunk` of `diff` still belongs: what came for an older diff would land among
   * the newer one's, and a chunk let go of is far from what shows. */
  #wanted(diff: Ready, chunk: number): boolean {
    return this.#diff(this.view) === this.#diff(diff) && this.#asked.has(chunk);
  }

  /** The commits the diff `view` shows runs between, or nothing if it shows none. */
  #diff(view: PrFiles | null | undefined): string | undefined {
    return view?.state === "ready" ? `${view.base}..${view.head}` : undefined;
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

/** The chunks that hold the diff's rows `pieces` cover, in order. */
function chunksOf(pieces: [number, number][]): number[] {
  const chunks: number[] = [];
  for (const [start, end] of pieces) {
    for (let chunk = Math.floor(start / CHUNK); chunk <= Math.floor((end - 1) / CHUNK); chunk++) {
      // The pieces come in order, so a chunk two of them share comes twice in a row.
      if (chunks.at(-1) !== chunk) chunks.push(chunk);
    }
  }
  return chunks;
}
