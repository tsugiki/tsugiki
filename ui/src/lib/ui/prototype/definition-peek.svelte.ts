// PROTOTYPE, throwaway (#365): Definition peek and Go to definition, three ways. Lives on the
// `prototype/definition-peek` branch only. The floating bar at the bottom switches the variant and what the
// lookup pretends: a mirror still cloning, a slow search, or no hit.

import { SvelteMap } from "svelte/reactivity";
import { commands, type HeadLine, type ProtoHit, type ProtoHits } from "$lib/data/bindings";

export type Variant = "A" | "B" | "C";
export const VARIANTS: { key: Variant; name: string }[] = [
  { key: "A", name: "Inline under the line" },
  { key: "B", name: "Split below the diff" },
  { key: "C", name: "Floating card at the symbol" },
];
export type Simulate = "real" | "slow" | "cloning" | "none";
export const SIMULATIONS: { key: Simulate; name: string }[] = [
  { key: "real", name: "Real git grep" },
  { key: "slow", name: "Slow search (1.5 s)" },
  { key: "cloning", name: "Mirror still cloning" },
  { key: "none", name: "No hit" },
];

/** A symbol the user asked about, and the line it came from. */
export interface Lookup {
  repository: string;
  number: number;
  word: string;
  commit: string;
  /** Which end of the diff the commit is: a removed line is looked up at the base. */
  which: "head" | "base";
  /** The file and line the symbol is on. */
  from: string;
  fromLine: number;
}

export type Answer =
  | { state: "searching" }
  | { state: "cloning" }
  | { state: "done"; hits: ProtoHits }
  | { state: "error"; error: string };

export interface Peek {
  lookup: Lookup;
  /** The diff's place the peek opened under (variant A). */
  place: number;
  /** Where the symbol is on screen (variant C). */
  rect: { left: number; top: number; bottom: number };
  answer: Answer;
  selected: number;
  /** The lookups an Alt+F12 inside the peek replaced, newest last, for its breadcrumb. */
  trail: Lookup[];
}

export interface FileView {
  lookup: Lookup;
  path: string;
  line: number;
  /** Whether the file is one the PR changes, though the line isn't in a hunk. */
  changed: boolean;
}

export type FileLines = { state: "loading" } | { state: "done"; lines: HeadLine[] } | { state: "error"; error: string };

const short = (commit: string) => commit.slice(0, 7);
export const commitWords = (lookup: Lookup) =>
  `${short(lookup.commit)} · ${lookup.which === "head" ? "the PR's head" : "the PR's base"}`;
export const fileKey = (commit: string, path: string) => `${commit}:${path}`;

const readParam = <T extends string>(name: string, allowed: readonly T[], fallback: T): T => {
  try {
    const value = new URLSearchParams(location.search).get(name) ?? localStorage.getItem(`proto-${name}`);
    return allowed.includes(value as T) ? (value as T) : fallback;
  } catch {
    return fallback;
  }
};

class DefinitionProto {
  variant = $state<Variant>(readParam("variant", ["A", "B", "C"], "A"));
  simulate = $state<Simulate>(readParam("simulate", ["real", "slow", "cloning", "none"], "real"));
  peek = $state<Peek>();
  fileView = $state<FileView>();
  /** A short line over the diff: "Searching…", "No definition…". */
  notice = $state<string>();
  #noticeTimer: ReturnType<typeof setTimeout> | undefined;
  files = new SvelteMap<string, FileLines>();
  /** The cloning progress the simulation shows. */
  cloned = $state(41);

  setVariant(variant: Variant) {
    this.variant = variant;
    this.#save("variant", variant);
  }

  setSimulate(simulate: Simulate) {
    this.simulate = simulate;
    this.#save("simulate", simulate);
  }

  #save(name: string, value: string) {
    try {
      localStorage.setItem(`proto-${name}`, value);
      const url = new URL(location.href);
      url.searchParams.set(name, value);
      history.replaceState(history.state, "", url);
    } catch {
      /* a prototype */
    }
  }

  say(words: string | undefined, forMs = 2500) {
    clearTimeout(this.#noticeTimer);
    this.notice = words;
    if (words !== undefined && forMs > 0)
      this.#noticeTimer = setTimeout(() => {
        this.notice = undefined;
      }, forMs);
  }

  /** The definitions of the lookup's symbol, as the simulation has them. */
  async find(lookup: Lookup): Promise<Answer> {
    if (this.simulate === "cloning") return { state: "cloning" };
    if (this.simulate === "slow") await new Promise((done) => setTimeout(done, 1500));
    if (this.simulate === "none")
      return { state: "done", hits: { hits: [], fallback: false, millis: 3, capped: false } };
    const answer = await commands.prototypeDefinitions(lookup.repository, lookup.commit, lookup.word, lookup.from);
    return answer.status === "ok"
      ? { state: "done", hits: answer.data }
      : { state: "error", error: JSON.stringify(answer.error) };
  }

  /** Reads the file `path` at `commit`, once. */
  load(repository: string, commit: string, path: string) {
    const key = fileKey(commit, path);
    if (this.files.has(key)) return;
    this.files.set(key, { state: "loading" });
    void commands.prototypeFile(repository, commit, path).then((answer) => {
      this.files.set(
        key,
        answer.status === "ok"
          ? { state: "done", lines: answer.data }
          : { state: "error", error: JSON.stringify(answer.error) },
      );
    });
  }

  async openPeek(lookup: Lookup, place: number, rect: Peek["rect"], trail: Lookup[] = []) {
    this.peek = { lookup, place, rect, answer: { state: "searching" }, selected: 0, trail };
    const answer = await this.find(lookup);
    if (this.peek?.lookup !== lookup) return;
    this.peek.answer = answer;
    if (answer.state === "done" && answer.hits.hits[0]) {
      const hit = answer.hits.hits[0];
      this.load(lookup.repository, lookup.commit, hit.path);
    }
  }

  select(index: number) {
    const peek = this.peek;
    if (peek?.answer.state !== "done") return;
    const hits = peek.answer.hits.hits;
    if (hits.length === 0) return;
    peek.selected = (index + hits.length) % hits.length;
    const hit = hits[peek.selected];
    if (hit) this.load(peek.lookup.repository, peek.lookup.commit, hit.path);
  }

  selectedHit(): ProtoHit | undefined {
    const peek = this.peek;
    return peek?.answer.state === "done" ? peek.answer.hits.hits[peek.selected] : undefined;
  }

  closePeek() {
    this.peek = undefined;
  }

  /** What opens a hit in full: the Files tab's diff or the file view. The host sets it. */
  opener: ((lookup: Lookup, hit: ProtoHit) => void) | undefined;

  openSelected() {
    const hit = this.selectedHit();
    if (this.peek && hit) this.opener?.(this.peek.lookup, hit);
  }

  reset() {
    this.peek = undefined;
    this.fileView = undefined;
    this.stack = [];
    this.ahead = [];
    this.notice = undefined;
  }

  /** The file views Alt+← goes back through, the one on show last, and those Alt+→ comes forward to. */
  stack = $state<FileView[]>([]);
  ahead = $state<FileView[]>([]);
  /** A row of the diff to mark for a moment after Go to definition jumped to it. */
  flash = $state<number>();

  openFile(lookup: Lookup, hit: ProtoHit, changed: boolean) {
    this.peek = undefined;
    const view = { lookup, path: hit.path, line: hit.line, changed };
    this.stack = [...this.stack, view];
    this.ahead = [];
    this.fileView = view;
    this.load(lookup.repository, lookup.commit, hit.path);
  }

  back() {
    const gone = this.stack.at(-1);
    if (!gone) return false;
    this.stack = this.stack.slice(0, -1);
    this.ahead = [gone, ...this.ahead];
    this.fileView = this.stack.at(-1);
    return true;
  }

  forward() {
    const next = this.ahead[0];
    if (!next) return false;
    this.ahead = this.ahead.slice(1);
    this.stack = [...this.stack, next];
    this.fileView = next;
    return true;
  }

  flashRow(row: number) {
    this.flash = row;
    setTimeout(() => {
      if (this.flash === row) this.flash = undefined;
    }, 1600);
  }
}

export const proto = new DefinitionProto();

/** The identifier around offset `at` of `text`, if there is one. */
export function wordAt(text: string, at: number): string | undefined {
  const isWord = (c: string | undefined) => c !== undefined && /[A-Za-z0-9_]/.test(c);
  let start = at;
  let end = at;
  while (start > 0 && isWord(text[start - 1])) start--;
  while (end < text.length && isWord(text[end])) end++;
  const word = text.slice(start, end);
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(word) ? word : undefined;
}

/** The symbol at the caret or the selection, and where it is on screen, if the caret is in `inside`. */
export function symbolAtCaret(
  inside: HTMLElement,
  point: { x: number; y: number } | undefined,
): { word: string; element: Element; rect: DOMRect } | undefined {
  const selection = getSelection();
  let range: Range | null = null;
  if (selection && selection.rangeCount > 0 && inside.contains(selection.anchorNode)) {
    range = selection.getRangeAt(0);
  } else if (point) {
    range = document.caretRangeFromPoint(point.x, point.y);
  }
  if (!range || !inside.contains(range.startContainer)) return undefined;
  const node = range.startContainer;
  const element = node instanceof Element ? node : node.parentElement;
  if (!element) return undefined;
  const text = node.textContent ?? "";
  const word = range.collapsed
    ? wordAt(text, range.startOffset)
    : /^[A-Za-z_][A-Za-z0-9_]*$/.test(range.toString().trim())
      ? range.toString().trim()
      : undefined;
  if (word === undefined) return undefined;
  return { word, element, rect: range.getBoundingClientRect() };
}
