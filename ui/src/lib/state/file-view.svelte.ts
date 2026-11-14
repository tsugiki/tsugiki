// The file view (#453): Go to definition into a file the PR doesn't change opens that file read-only over the
// Files tab, at the hit's commit, with the hit's line marked. Its lines come a chunk at a time, as the
// Definition peek's code does, so a big file opens without the window waiting for all of it. The user follows
// symbols inside it as in the diff (#457): Go to definition from it lands in it, in another file view or in
// the diff, and the Definition peek opens under one of its lines.

import type { FileChange, Hit, Place } from "$lib/data/bindings";
import type { HitCode } from "./hit-code.svelte";
import { sameStop, type Stop } from "./workbench.svelte";

/** A place in a file of the Repository, at a commit. */
type InRepository = Extract<Place, { kind: "repository" }>;

/** A hit in a file of the Repository. */
export type RepositoryHit = Hit & { place: InRepository };

/** Whether `hit` is in a file of the Repository, and not one on disk outside it. */
export function inRepository(hit: Hit): hit is RepositoryHit {
  return hit.place.kind === "repository";
}

/** The file a hit is in, at the hit's commit, its lines, and the line the view marks. */
export class FileView {
  readonly place: InRepository;
  readonly code: HitCode;
  /** The line the view marks, counted from 1: the hit's line it opened on, then the line Go to definition
   * last landed on in it (#457). Each landing is a new one, so the view scrolls to it again on the same
   * line. */
  landing = $state.raw<{ line: number }>({ line: 1 });

  constructor(place: InRepository, line: number, code: HitCode) {
    this.place = place;
    this.code = code;
    this.landing = { line };
  }

  /** Lands Go to definition on line `line` at column `column`, in UTF-16 code units: the view marks the line
   * and scrolls to it from wherever the user left it, and the caret moves to the word there, so the next
   * `F12` looks up from it. A line not read yet takes no pick. */
  land(line: number, column: number) {
    this.code.scrollTop = undefined;
    this.code.pickWord(line, column, false);
    this.landing = { line };
  }
}

/** A line of the file a file view shows, counted from 1, which a Definition peek opens under (#457). */
export type ViewLine = { kind: "view"; view: FileView; line: number };

/** A place in the Detail pane's history (#456): an Issue or a PR on its line of the list, or a file view,
 * which belongs to the line of the PR it shows over. */
export type HistoryPlace = Stop | { kind: "file"; line: Stop; view: FileView };

/** Whether two places are one: the same line of the list, or the same file view. */
export function samePlace(one: HistoryPlace, other: HistoryPlace): boolean {
  if (one.kind !== "file" && other.kind !== "file") return sameStop(one, other);
  return one.kind === "file" && other.kind === "file" && one.view === other.view;
}

/** The line of the list `place` belongs to, which takes the cursor while it shows. */
export const lineOf = (place: HistoryPlace): Stop => (place.kind === "file" ? place.line : place);

/** Whether the diff of `files` shows lines of the file `place` names: at the base a renamed file goes by
 * its old path, and at the head by its new one. A file the PR only renames, or whose mode alone changes,
 * shows none, so the file view shows it. */
export function diffShows(
  files: readonly { path: string; old_path?: string | null; change: FileChange }[],
  place: InRepository,
): boolean {
  return files.some(
    (file) =>
      (place.side === "base" ? (file.old_path ?? file.path) : file.path) === place.path &&
      file.change.kind === "lines" &&
      file.change.added + file.change.removed > 0,
  );
}

/** What the file view's header says, for PR `number`. */
export function fileViewWords(view: FileView, number: number): { back: string; commit: string } {
  const pr = `PR #${String(number)}`;
  const side = view.place.side === "base" ? "the PR's base" : "the PR's head";
  return {
    back: `← Back to ${pr}'s diff`,
    commit: `at ${view.place.commit.slice(0, 7)} · ${side} of ${pr}`,
  };
}
