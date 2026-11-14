// The Definition peek (#438): under the line of the word the user picked, where that word is defined at
// the line's commit. It takes places in the diff's layout as a review thread does, so the rows after it
// move down.

import type { Answer, Hit, Index_Deserialize, ServerName, Waiting } from "$lib/data/bindings";
import type { LaidThread } from "./diff-layout";
import type { HitCode } from "./hit-code.svelte";
import type { PickedLine, Side } from "./pick";

/** What a lookup found: nothing yet, the server's answer, or why it failed. While it looks, the core says
 * what it waits for (#440), and `since` is when it started to. */
export type Lookup =
  | { state: "looking"; waiting?: Waiting; since?: number }
  | { state: "answered"; answer: Answer }
  | { state: "failed"; problem: string };

/** The news of the index of the server that answered, which the core says on a channel of the lookup's own
 * (#680). The bindings name the type for the window's side of the channel. */
export type IndexNews = Index_Deserialize;

/** A server that builds its index, and how many files it has indexed, once it has said. */
export type Indexing = Extract<IndexNews, { state: "building" }>;

/** How the peek names each server, and the language it answers in. */
const SERVERS: Record<ServerName, { name: string; language: string }> = {
  ty: { name: "ty", language: "Python" },
  "rust-analyzer": { name: "rust-analyzer", language: "Rust" },
  clangd: { name: "clangd", language: "C" },
};

/** What a lookup that waits says: the server starts, or works out the answer, with the seconds it has
 * waited at `now` once they pass one. */
function waitingWords(symbol: string, lookup: { waiting?: Waiting; since?: number }, now: number): string {
  const waiting = lookup.waiting;
  if (waiting === undefined) return `Searching for the definition of ${symbol}…`;
  const server = SERVERS[waiting.server].name;
  if (waiting.step === "starting") return `Starting ${server}…`;
  const seconds = Math.floor((now - (lookup.since ?? now)) / 1000);
  const waited = seconds >= 1 ? ` ${String(seconds)} s` : "";
  return `Asking ${server} for the definition of ${symbol}…${waited}`;
}

/** What the peek's header and the line under it say. */
export interface PeekWords {
  /** While there are hits: the lookup's `hits`, which the peek lists on its right, and the `hit` it shows,
   * which is `hits[shown]`. It shows the first until the user picks another (#441). */
  found?: { hits: readonly Hit[]; shown: number; hit: Hit };
  /** Where the hit is: its path and line. */
  where?: string;
  /** The commit it is at, as "at abc1234 · the PR's head", or the lookup's while it has no hit. A hit
   * outside the Repository has none, and its path is the whole path on disk. */
  commit?: string;
  /** "3 of N" while there are hits, for the third. */
  count?: string;
  /** The line under the header: the hit's text, or what the lookup is doing or found. */
  body: string;
  /** A line between the header and the rest, while the server that answered builds its index (#680). */
  indexing?: string;
}

/** The line the peek shows while the server that answered builds its index: the answer may be a
 * declaration. */
function indexingWords(indexing: Indexing): string {
  const files = indexing.files;
  const counted = files ? ` (${String(files.done)} of ${String(files.total)} files)` : "";
  return `${SERVERS[indexing.server].name} is indexing${counted}: this may be a declaration`;
}

/** The words `peek` shows at `now`, which counts a lookup's seconds. */
export function peekWords(peek: Peek, now = Date.now()): PeekWords {
  const at = (commit: string, side: Side) =>
    `at ${commit.slice(0, 7)} · ${side === "base" ? "the PR's base" : "the PR's head"}`;
  const asked = at(peek.commit, peek.side);
  const lookup = peek.lookup;
  if (lookup.state === "looking") return { commit: asked, body: waitingWords(peek.symbol, lookup, now) };
  if (lookup.state === "failed") return { commit: asked, body: lookup.problem };
  const answer = lookup.answer;
  if (answer.state === "not_set_up") {
    const { name, language } = SERVERS[answer.server];
    return { commit: asked, body: `Set up ${name} to go to definitions in ${language}` };
  }
  if (answer.state === "no_server") {
    return { commit: asked, body: "Tsugiki can't go to definitions in this file's language yet" };
  }
  // The window drops the answer of each lookup it cancels, so this shows only for one it didn't.
  if (answer.state === "cancelled") return { commit: asked, body: "The lookup was cancelled" };
  const indexing = peek.indexing && { indexing: indexingWords(peek.indexing) };
  const shown = peek.code?.index ?? 0;
  const hit = answer.hits[shown];
  if (hit === undefined) return { commit: asked, body: `No definition of ${peek.symbol}`, ...indexing };
  const place = hit.place;
  return {
    found: { hits: answer.hits, shown, hit },
    where: `${place.path}:${String(hit.line)}`,
    ...(place.kind === "repository" && { commit: at(place.commit, place.side) }),
    count: `${String(shown + 1)} of ${String(answer.hits.length)}`,
    body: hit.text,
    ...indexing,
  };
}

/** The peek: the line it opened under, the symbol it looks up, and the commit it looks at, which is the
 * PR's base for a removed line and its head for the rest. With hits, it holds the code of the one shown. */
export interface Peek {
  at: PickedLine;
  symbol: string;
  side: Side;
  commit: string;
  lookup: Lookup;
  code?: HitCode;
  /** The server that answered builds its index, so the answer may miss what the index doesn't hold yet. */
  indexing?: Indexing;
}

/** The id that names the peek among the threads of the diff's layout. A thread's id is GitHub's, which
 * never has a space. */
export const PEEK_ID = "definition peek";

/** The rows the peek takes while it looks a name up or shows a hit's code, until the user drags its bottom
 * edge (#458): its header and the code under it, which scrolls inside it (#439). */
export const PEEK_ROWS = 18;

/** Whether `peek` takes the rows the user sizes, as while it looks up or shows code. With only words to
 * say, as when its only hit's file can't be read, it takes its header with a line under it. With several
 * hits it keeps its rows, so the list of them stays to pick another. */
export function peekSized(peek: Peek): boolean {
  const lookup = peek.lookup;
  const answer = lookup.state === "answered" ? lookup.answer : undefined;
  const hits = answer?.state === "found" ? answer.hits.length : 0;
  return !(
    lookup.state === "failed" ||
    (lookup.state === "answered" && hits === 0) ||
    (peek.code?.problem !== undefined && hits < 2)
  );
}

/** The places `peek` takes in the diff's layout: `rows` while `peekSized` says it takes the rows the user
 * sizes, and else its header with a line under it. The line that says the server indexes takes one more
 * place with the words. */
export function peekPlaces(peek: Peek, rows: number): number {
  if (peekSized(peek)) return rows;
  return peek.indexing === undefined ? 2 : 3;
}

/** `threads`, in the order they show, with `peek` among them right under its line, above any thread there.
 * The layout places threads under rows and threads under lines apart, so only those of the peek's kind
 * order it. */
export function withPeek<T extends LaidThread>(threads: readonly T[], peek: T & { at: PickedLine }): T[] {
  const at = peek.at;
  const after = threads.findIndex((thread) => {
    const other = thread.at;
    if (at.kind === "row") return other.kind === "row" && other.row >= at.row;
    return other.kind === "line" && other.file === at.file && other.line >= at.line;
  });
  const place = after === -1 ? threads.length : after;
  return [...threads.slice(0, place), peek, ...threads.slice(place)];
}
