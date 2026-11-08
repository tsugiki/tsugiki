// A PR's Files tab: its diff from the Repository's mirror, or why it isn't there yet. The rows stay in the
// core, and this asks for them in chunks near what the tab shows (ADR 0001), with the spans that highlight
// them (#178). A chunk's rows wait a moment for their spans, so a jump into a file the core has parsed shows
// it highlighted at once (#180). Generated files start collapsed, and their rows aren't asked for until the
// user opens one (#181). The user can expand the lines a gap between hunks leaves out, which the core reads
// from the file at the PR's head (#182), and switch between the unified and side-by-side views (#183).
// Review threads sit under their lines, and a resolved one starts folded to a line (#184). A thread on a
// whole file or an Outdated one sits at the top of its file, a thread on a line no hunk shows opens the lines
// around it, and those on files no longer in the diff come after the last file (#185). A fetch of the PR's
// head that fails says why, until the tab asks again, which hands the head to the mirror once more (#233).
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import {
  commands,
  events,
  type Cloning,
  type CodeRow,
  type DiffFile_Serialize,
  type DiffRow_Serialize,
  type DiffThread_Serialize,
  type FileChange,
  type Found,
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
import {
  DiffLayout,
  type DiffView,
  type HiddenLines,
  type Lines,
  type Shown,
  type ThreadAnchor,
} from "./diff-layout";
import { hud, type Jump, type JumpTimer } from "./hud.svelte";

export type PrFiles = PrFiles_Serialize;
export type DiffFile = DiffFile_Serialize;
export type DiffRow = DiffRow_Serialize;
export type NewsEntry = NewsEntry_Serialize;
export type DiffThread = DiffThread_Serialize;
/** A diff the tab shows, from its merge base to its head. */
type Ready = Extract<PrFiles, { state: "ready" }>;
export type {
  Cloning,
  CodeRow,
  DiffView,
  FileChange,
  Gap,
  HeadLine,
  HiddenLines,
  Highlight,
  LinkTo,
  NewsBlock,
  NewsPiece,
  Shown,
  Span,
};

/** Rows asked for at a time: about 25 screens, as the diff prototype found best. */
export const CHUNK = 512;

/** Lines a click shows above or below a gap, as on GitHub. */
export const EXPAND = 20;

/** Lines that show on either side of a line no hunk shows, under which a thread sits, as on GitHub. */
export const AROUND = 3;

/** The rows of code an Outdated thread shows before the user asks for its whole hunk. */
export const CODE_ROWS = 4;

/** The tab's guess at an open thread's places before it measures the thread: its header, the code it was
 * written against, and a line of who wrote each comment and two of its words. */
const guessPlaces = (thread: DiffThread) => {
  const code = "code" in thread.at && thread.at.code !== null ? CODE_ROWS + 1 : 0;
  return 1 + code + 3 * thread.comments.length;
};

/** The line past any file's last, to ask for a file to its end. */
const END = 2 ** 32 - 1;

/** How long a chunk's rows wait for their spans before they show plain. The spans repaint the chunk when
 * they come. */
export const SPANS_WAIT = 50;

/** The view the Files tab shows diffs in. A Repository's page keeps one for all its PRs, so the view stays
 * as the user moves from PR to PR. */
export class DiffViewChoice {
  view = $state<DiffView>("unified");

  /** Switches to the other view. */
  toggle() {
    this.view = this.view === "unified" ? "side-by-side" : "unified";
  }
}

export class PrFilesState {
  /** Undefined until the core answers, and null if Tsugiki doesn't hold this PR. */
  view = $state<PrFiles | null>();
  error = $state<string>();
  /** The mirror's clone under way, which a Repository's first diff waits for. */
  cloning = $state<Cloning | null>(null);
  /** git's words for why the mirror's fetch of the PR's head failed, kept until the tab asks again. */
  fetchFailed = $state<string>();
  /** The rows that show, and where: a Generated file the user hasn't opened shows only its header, and a
   * gap the user expanded shows the lines it left out. */
  layout = $derived.by(() => {
    const view = this.view;
    const ready = view?.state === "ready";
    const files = ready ? view.files : [];
    return new DiffLayout(
      ready ? view.rows : 0,
      files,
      (file) => this.collapsed(file),
      { gap: (file) => this.#held.get(file) ?? [], rest: (file, rest) => this.#after(file, rest) },
      this.#choice.view,
      this.threads.map((thread, at) => ({
        at: anchor(thread, files),
        places: this.#places(thread, at),
        id: thread.id,
        thread,
      })),
      this.#diff(view) ?? "",
    );
  });
  /** The thread or the diff's row to scroll to, until the diff has scrolled there. Each move is a new one,
   * so a move back to the same place scrolls again. */
  jump = $state<{ id: string } | { row: number }>();
  /** Whether the search in the diff shows, which Ctrl+F opens (#186). */
  searching = $state(false);
  /** The text the user searches the diff for. */
  query = $state("");
  /** Whether Ctrl+F has asked the search box for focus, until the box takes it. */
  searchWantsFocus = $state(false);
  /** Whether the search has closed and handed the diff its focus back, until the diff takes it. */
  diffWantsFocus = $state(false);
  /** Why the core couldn't search, which the search box says. */
  searchProblem = $state<string>();
  /** How many characters the longest expanded line has, so the tab makes room for it as for the diff's. */
  columns = $state(0);
  readonly #repository: string;
  readonly #number: number;
  readonly #jumps: JumpTimer;
  readonly #choice: DiffViewChoice;
  /** The rows the core has sent, by chunk, for the diff `view` shows. */
  #chunks = new SvelteMap<number, DiffRow[]>();
  /** The spans of each row, by chunk, which come after its rows. */
  #spans = new SvelteMap<number, Span[][]>();
  /** The Generated files the user has opened, by their place among the files. */
  #opened = new SvelteSet<number>();
  /** The lines expanded gaps show, by `file:line` at the head, once the core sends them. A gap shows the
   * lines held, so a click that fails shows nothing, and two that overlap ask for the same lines. */
  #lines = new SvelteMap<string, HeadLine>();
  /** The runs of lines held, by the file's place, in order, which the layout shows where gaps and the ends
   * of files leave them out. */
  #held = new SvelteMap<number, Lines[]>();
  /** How many lines each file has at the head, by its place, once an answer stopped short at its end. */
  #lengths = new SvelteMap<number, number>();
  /** The threads the user folded or opened, against how each starts: open, or folded once resolved. */
  #flipped = new SvelteSet<string>();
  /** The places of each open thread the tab has measured, by its id. */
  #measured = new SvelteMap<string, number>();
  /** The thread to move to once the diff comes, as one opened from the Conversation before it did. */
  #revealing: string | undefined;
  /** The thread the tab last moved to, which F8 goes on from. */
  #current: string | undefined;
  /** The row of the change the tab last moved to, which F7 goes on from. */
  #change: number | undefined;
  /** What the search found in the diff `view` shows, and the files of that diff. The matches are many and
   * don't change, so they aren't deeply reactive. */
  #found = $state.raw<{ found: Found; files: DiffFile[] }>();
  /** Which match the tab is on: none when the search found nothing. */
  #at = $state<number>();
  /** The row of the match the tab last moved to, which a new query moves on from. */
  #matchRow: number | undefined;
  /** The matches in each row, by the row, with each one's place among the matches. */
  #byRow = $derived.by(() => {
    // Made anew whenever the matches change, so it needn't be reactive itself.
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const byRow = new Map<number, { start: number; end: number; index: number }[]>();
    this.#found?.found.matches.forEach(({ row, start, end }, index) => {
      const matches = byRow.get(row) ?? [];
      matches.push({ start, end, index });
      byRow.set(row, matches);
    });
    return byRow;
  });
  #lastSearch = 0;
  /** The chunks asked for. Nothing draws from it, and the effect that asks shouldn't rerun on it. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #asked = new Set<number>();
  #lastQuestion = 0;
  #stopListening: (() => void)[] = [];
  #stopped = false;

  constructor(
    repository: string,
    number: number,
    jumps: JumpTimer = hud,
    choice: DiffViewChoice = new DiffViewChoice(),
  ) {
    this.#repository = repository;
    this.#number = number;
    this.#jumps = jumps;
    this.#choice = choice;
  }

  /** Whether the tab shows the diff unified or side by side. */
  get shownAs(): DiffView {
    return this.#choice.view;
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
        events.headsFetchFailed.listen(({ payload }) => {
          if (mine(payload.repository, payload.items)) this.fetchFailed = payload.why;
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
   * holds. The core hands a head the mirror lacks to the mirror again as it answers, so this is also how
   * the user tries again after a fetch failed. */
  async ask() {
    // The core hands the head to the mirror again, so a failure heard before is of a fetch that's over. One
    // the mirror sent before this can land after it, and shows until the next answer or failure.
    this.fetchFailed = undefined;
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await this.#safely(() => commands.pullRequestFiles(this.#repository, this.#number));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion || answer === undefined) return;
    const anew = this.#diff(answer) !== this.#diff(this.view);
    if (anew) {
      this.#chunks.clear();
      this.#spans.clear();
      this.#asked.clear();
      this.#opened.clear();
      this.#lines.clear();
      this.#held.clear();
      this.#lengths.clear();
      this.columns = 0;
      this.#change = undefined;
      // The matches name rows of the diff before.
      this.#found = undefined;
      this.#at = undefined;
      this.#matchRow = undefined;
    }
    // The threads the tab showed with the same diff, whose lines it has opened.
    const before = anew ? [] : this.threads;
    this.view = answer;
    // A thread on a line no hunk shows opens the lines around it, once.
    for (const thread of this.threads) {
      if (thread.at.kind !== "outside" || before.some((shown) => shown.id === thread.id)) continue;
      const { file, head_line: line } = thread.at;
      void this.#fetchLines(file, Math.max(1, line - AROUND), line + AROUND + 1);
    }
    if (this.#revealing !== undefined && answer?.state === "ready") this.reveal(this.#revealing);
    // The rows of a new diff hold other matches, and the tab stays where the user is.
    if (anew && this.query !== "") void this.#search(this.query, false);
  }

  /** The review threads, in the order the tab shows them. */
  get threads(): DiffThread[] {
    const view = this.view;
    return view?.state === "ready" && view.threads.kind === "shown" ? view.threads.threads : [];
  }

  /** The PR's newer head the mirror is fetching, and how many threads wait for its diff, while the tab
   * shows the diff before. */
  get heldBack(): { newer: string; count: number } | undefined {
    const view = this.view;
    if (view?.state !== "ready" || view.threads.kind !== "held_back") return undefined;
    const { newer, count } = view.threads;
    return { newer, count };
  }

  /** Whether thread `thread`, by its place among the threads, shows only its first line. */
  folded(thread: number): boolean {
    const shown = this.threads[thread];
    return shown !== undefined && (shown.resolution === "resolved") !== this.#flipped.has(shown.id);
  }

  /** Folds thread `thread`, by its place among the threads, or opens it. */
  toggleThread(thread: number) {
    const id = this.threads[thread]?.id;
    if (id !== undefined && !this.#flipped.delete(id)) this.#flipped.add(id);
  }

  /** Notes that open thread `id` takes `places` places, as the tab measured it. */
  measured(id: string, places: number) {
    if (this.#measured.get(id) !== places) this.#measured.set(id, places);
  }

  /** The places thread `thread`, at `at` among the threads, takes: one when folded. */
  #places(thread: DiffThread, at: number): number {
    return this.folded(at) ? 1 : (this.#measured.get(thread.id) ?? guessPlaces(thread));
  }

  /** Moves to thread `id`, open, and opens the Generated file it is in. Before the diff comes, or while the
   * older diff of a moved head holds the threads back, it moves there once the threads come. A thread the tab
   * doesn't show stays where it is. */
  reveal(id: string) {
    const view = this.view;
    if (view?.state !== "ready" || view.threads.kind === "held_back") {
      this.#revealing = id;
      return;
    }
    this.#revealing = undefined;
    const thread = this.threads.findIndex((shown) => shown.id === id);
    const at = this.threads[thread]?.at;
    if (at === undefined) return;
    if (this.folded(thread)) this.toggleThread(thread);
    const file = at.kind === "row" ? fileOf(view.files, at.row) : "file" in at ? at.file : -1;
    if (this.collapsed(file)) this.toggle(file);
    this.#current = id;
    this.jump = { id };
  }

  /** Moves to the next unresolved thread after the one the tab last moved to, or by `step` -1 to the one
   * before, round from the end to the start. Without one, or once a new diff dropped it, it starts from the
   * end it moves away from. */
  nextThread(step: 1 | -1) {
    const threads = this.threads;
    const open = threads.flatMap((thread, at) => (thread.resolution === "open" ? [at] : []));
    const current = threads.findIndex((thread) => thread.id === this.#current);
    const from = current !== -1 ? current : step === 1 ? -1 : threads.length;
    const next = cycle(open, from, step);
    const id = next === undefined ? undefined : threads[next]?.id;
    if (id !== undefined) this.reveal(id);
  }

  /** Moves to the change after the one the tab last moved to, or by `step` -1 to the one before, round from
   * the end to the start, and opens the Generated file it is in. Without one, it starts from the end it
   * moves away from. */
  nextChange(step: 1 | -1) {
    const view = this.view;
    if (view?.state !== "ready") return;
    const rows = view.files.flatMap((file) => file.pairs.map((pair) => pair.row));
    const row = cycle(rows, this.#change ?? (step === 1 ? -1 : Infinity), step);
    if (row === undefined) return;
    this.#change = row;
    this.#moveTo(view.files, row);
  }

  /** Shows the search, or gives its box focus again. */
  openSearch() {
    this.searching = true;
    this.searchWantsFocus = true;
  }

  /** Hides the search, forgets its text and its matches, and hands the diff its focus back. */
  closeSearch() {
    this.searching = false;
    this.query = "";
    this.#found = undefined;
    this.#at = undefined;
    this.#matchRow = undefined;
    this.searchProblem = undefined;
    this.#lastSearch += 1;
    this.diffWantsFocus = true;
  }

  /** Searches every row of the diff for `query`, whatever the case, through the core, which holds the rows
   * the tab doesn't, as the user types. It moves to the match it was on, or else the first after it. */
  find(query: string): Promise<void> {
    return this.#search(query, true);
  }

  /** Searches for `query`, and moves to the match it lands on when `move` says so. */
  async #search(query: string, move: boolean) {
    this.query = query;
    this.#lastSearch += 1;
    const asked = this.#lastSearch;
    const view = this.view;
    if (query === "" || view?.state !== "ready") {
      this.#found = undefined;
      this.#at = undefined;
      return;
    }
    const { head, base } = view;
    const search = { repository: this.#repository, number: this.#number, head, base, query };
    // The search's own problem shows in its box, and leaves the tab's error to the diff.
    const answer = await commands.pullRequestFind(search).catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    // A newer query, or a closed search, drops an older answer that arrives late.
    if (asked !== this.#lastSearch) return;
    if (answer.status === "error") {
      this.searchProblem = answer.error.message;
      this.#found = undefined;
      this.#at = undefined;
      return;
    }
    const found = answer.data;
    this.searchProblem = undefined;
    const after = found.matches.findIndex((match) => match.row >= (this.#matchRow ?? 0));
    this.#found = { found, files: view.files };
    this.#at = found.matches.length === 0 ? undefined : Math.max(0, after);
    if (move) this.#moveToMatch();
  }

  /** How many matches the search found, more when the core stopped counting, and which the tab is on. */
  get search(): { at: number | undefined; count: number; more: boolean } | undefined {
    const found = this.#found?.found;
    return found && { at: this.#at, count: found.matches.length, more: found.more };
  }

  /** Where the search found its text in the diff's row `row`, and which is the match the tab is on. */
  foundIn(row: number): { start: number; end: number; current: boolean }[] {
    const at = this.#at;
    return (this.#byRow.get(row) ?? []).map(({ start, end, index }) => ({
      start,
      end,
      current: index === at,
    }));
  }

  /** Moves to the next match, or by `step` -1 to the one before, round from the end to the start. */
  nextMatch(step: 1 | -1) {
    const count = this.#found?.found.matches.length;
    if (count === undefined || this.#at === undefined) return;
    this.#at = (this.#at + step + count) % count;
    this.#moveToMatch();
  }

  #moveToMatch() {
    const found = this.#found;
    const match = this.#at === undefined ? undefined : found?.found.matches[this.#at];
    if (found === undefined || match === undefined) return;
    this.#matchRow = match.row;
    this.#moveTo(found.files, match.row);
  }

  /** Scrolls to the diff's row `row`, and opens the Generated file among `files` it is in. */
  #moveTo(files: DiffFile[], row: number) {
    const file = fileOf(files, row);
    if (this.collapsed(file)) this.toggle(file);
    this.jump = { row };
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

  /** The lines that show after file `file`'s last hunk, from `rest` on, and whether the file ends with
   * them: once an answer stopped short at its end, and its last line is held. */
  #after(file: number, rest: Rest): { runs: readonly Lines[]; ended: boolean } {
    const runs = this.#held.get(file) ?? [];
    const length = this.#lengths.get(file);
    const ended =
      length !== undefined && runs.some((run) => run.from <= length && length < run.from + run.length);
    return { runs: runs.filter((run) => run.from + run.length > rest.new), ended };
  }

  /** Shows `EXPAND` more of the lines `hidden`, from its bottom up. */
  expandAbove(hidden: HiddenLines) {
    const end = hidden.new + hidden.lines;
    void this.#fetchLines(hidden.file, Math.max(hidden.new, end - EXPAND), end);
  }

  /** Shows `EXPAND` more of the lines `hidden`, from its top down. */
  expandBelow(hidden: HiddenLines) {
    void this.#fetchLines(hidden.file, hidden.new, Math.min(hidden.new + EXPAND, hidden.new + hidden.lines));
  }

  /** Shows every line of `hidden`. */
  expandGap(hidden: HiddenLines) {
    void this.#fetchLines(hidden.file, hidden.new, hidden.new + hidden.lines);
  }

  /** Shows `EXPAND` more lines after the last that shows after file `file`'s last hunk. Fewer than asked
   * for end the file, and the button goes. */
  async expandRest(file: number) {
    const view = this.view;
    const rest = view?.state === "ready" ? view.files[file]?.rest : undefined;
    if (rest === undefined || rest === null) return;
    const last = this.#after(file, rest).runs.at(-1);
    const start = last === undefined ? rest.new : last.from + last.length;
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
    const held = (this.#held.get(file) ?? []).some(
      (run) => run.from <= start && end <= run.from + run.length,
    );
    if (view?.state !== "ready" || start >= end || held) return;
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
    if (lines.length > 0)
      this.#held.set(file, joined(this.#held.get(file) ?? [], { from: start, length: lines.length }));
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

/** The item of `items` after `from`, or by `step` -1 the one before, round from the end to the start: the
 * thread F8 moves to, or the change F7 does. Nothing, for no items. */
function cycle(items: readonly number[], from: number, step: 1 | -1): number | undefined {
  return step === 1
    ? (items.find((at) => at > from) ?? items[0])
    : (items.findLast((at) => at < from) ?? items.at(-1));
}

/** The place among `files` of the file a code row `row` of the diff is in. */
function fileOf(files: readonly DiffFile[], row: number): number {
  return files.findLastIndex((file) => file.row < row);
}

/** Where `thread` sits among the diff's `files`: under its row, under its file's header when it is on the
 * whole file or Outdated, under its line at the head when no hunk shows it, or after the last file. */
function anchor(thread: DiffThread, files: readonly DiffFile[]): ThreadAnchor {
  const at = thread.at;
  switch (at.kind) {
    case "row":
      return { kind: "row", row: at.row };
    case "file":
    case "outdated":
      // At the top, should a payload name a file the diff doesn't have.
      return { kind: "row", row: files[at.file]?.row ?? 0 };
    case "outside":
      return { kind: "line", file: at.file, line: at.head_line };
    case "gone":
      return { kind: "end" };
  }
}

/** `runs` with `added` among them, in order, as one with any it overlaps or follows on from. */
function joined(runs: readonly Lines[], added: Lines): Lines[] {
  const all = [...runs, added].sort((one, other) => one.from - other.from);
  const merged: Lines[] = [];
  for (const run of all) {
    const before = merged.at(-1);
    if (before !== undefined && before.from + before.length >= run.from)
      before.length = Math.max(before.length, run.from + run.length - before.from);
    else merged.push({ ...run });
  }
  return merged;
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
