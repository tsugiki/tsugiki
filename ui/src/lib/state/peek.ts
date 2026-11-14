// The Definition peek (#438): under the line of the word the user picked, where that word is defined at
// the line's commit. It takes places in the diff's layout as a review thread does, so the rows after it
// move down.

import type { Answer, Hit } from "$lib/data/bindings";
import type { LaidThread } from "./diff-layout";
import type { PickedLine, Side } from "./pick";

/** What a lookup found: nothing yet, ty's answer, or why it failed. */
export type Lookup =
  { state: "looking" } | { state: "answered"; answer: Answer } | { state: "failed"; problem: string };

/** What the peek's header and the line under it say. */
export interface PeekWords {
  /** The first hit, which the peek shows. */
  hit?: Hit;
  /** Where the hit is: its path and line. */
  where?: string;
  /** The commit it is at, as "at abc1234 · the PR's head", or the lookup's while it has no hit. */
  commit: string;
  /** "1 of N" while there are hits. */
  count?: string;
  /** The line under the header: the hit's text, or what the lookup is doing or found. */
  body: string;
}

/** The words `peek` shows. */
export function peekWords(peek: Peek): PeekWords {
  const at = (commit: string, side: Side) =>
    `at ${commit.slice(0, 7)} · ${side === "base" ? "the PR's base" : "the PR's head"}`;
  const asked = at(peek.commit, peek.side);
  const lookup = peek.lookup;
  if (lookup.state === "looking") {
    return { commit: asked, body: `Searching for the definition of ${peek.symbol}…` };
  }
  if (lookup.state === "failed") return { commit: asked, body: lookup.problem };
  const answer = lookup.answer;
  if (answer.state === "not_set_up")
    return { commit: asked, body: "Set up ty to go to definitions in Python" };
  if (answer.state === "no_server") {
    return { commit: asked, body: "Tsugiki can't go to definitions in this file's language yet" };
  }
  const hit = answer.hits[0];
  if (hit === undefined) return { commit: asked, body: `No definition of ${peek.symbol}` };
  const place = hit.place;
  return {
    hit,
    where: `${place.path}:${String(hit.line)}`,
    commit: place.kind === "repository" ? at(place.commit, place.side) : "outside the Repository",
    count: `1 of ${String(answer.hits.length)}`,
    body: hit.text,
  };
}

/** The peek: the line it opened under, the symbol it looks up, and the commit it looks at, which is the
 * PR's base for a removed line and its head for the rest. */
export interface Peek {
  at: PickedLine;
  symbol: string;
  side: Side;
  commit: string;
  lookup: Lookup;
}

/** The id that names the peek among the threads of the diff's layout. A thread's id is GitHub's, which
 * never has a space. */
export const PEEK_ID = "definition peek";

/** The places the peek takes: its header and a line under it. The hit's code comes in #439. */
export const PEEK_PLACES = 2;

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
