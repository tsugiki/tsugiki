// Quick open's search of the open Repository's Issues and PRs in the cache (#557). The core does the search
// and puts the best first, and the window holds its answer to the latest text typed. With a PR on show,
// quick open finds the files it changes too, which the window holds in its Files tab's diff (#558).
import { commands, type TitledItem_Serialize } from "$lib/data/bindings";
import { ask } from "./answer";

/** An Issue or a PR that quick open lists, named here for the page, which the UI's layers keep from the
 * bindings (`ui/eslint/layers.ts`). */
export type QuickOpenItem = TitledItem_Serialize;

/** The core's answer to one text typed: what it found, or why it couldn't search. */
export type QuickOpenAnswer = { typed: string } & ({ found: QuickOpenItem[] } | { problem: string });

/** The files the PR on show as quick open opened changes, which quick open finds (#558). */
export interface ChangedFiles {
  /** Reads the PR's diff, unless it has been read, as the rail's Search does. */
  readonly load: () => void;
  /** Their paths, in the order the Files tab shows them, once its diff is ready. Undefined while the core
   * hasn't answered the first read of the diff yet, and none for any other answer, or once the PR has left
   * the Detail pane. */
  readonly paths: () => readonly string[] | undefined;
  /** Opens the file at `path` in the Files tab, while the PR is still on show. */
  readonly open: (path: string) => void;
}

/** The most files quick open lists, as many as the Issues and PRs the core lists. */
export const FILES_LISTED = 20;

/** Whether quick open looks for files for `typed`: a text with something in it that doesn't start with `#`,
 * which names an Issue or a PR. Nothing typed lists the Issues and PRs alone, so a Ctrl+P for one of them
 * reads no diff. */
export function looksForFiles(typed: string): boolean {
  const text = typed.trim();
  return text !== "" && !text.startsWith("#");
}

/** The number `typed` names as a whole, as `123`, `#123` or `gh-123`, as the core reads it: the Issue or PR
 * with that number comes first. */
export function numberNamed(typed: string): number | undefined {
  const digits = /^(?:#|gh-)?(\d+)$/i.exec(typed.trim())?.[1];
  return digits === undefined ? undefined : Number(digits);
}

/** The paths of `files` that hold every word of `typed`, in any case, in the order the Files tab shows them,
 * `FILES_LISTED` at most, or undefined while the diff that holds them is on its way. None for a text quick
 * open looks for no files for. */
export function filesFound(files: ChangedFiles, typed: string): string[] | undefined {
  if (!looksForFiles(typed)) return [];
  const words = typed.toLowerCase().split(/\s+/);
  return files
    .paths()
    ?.filter((path) => {
      const lower = path.toLowerCase();
      return words.every((word) => lower.includes(word));
    })
    .slice(0, FILES_LISTED);
}

export class QuickOpenState {
  readonly #repository: string;
  /** The files of the PR on show as quick open opened, if a PR showed. */
  readonly files: ChangedFiles | undefined;
  /** The answer to the last search that came back. */
  answer = $state<QuickOpenAnswer>();
  /** The text of the search under way, if one is. */
  #searching: string | undefined;
  /** The latest text typed while a search was under way, which goes next. */
  #next: string | undefined;

  constructor(repository: string, files?: ChangedFiles) {
    this.#repository = repository;
    this.files = files;
  }

  /**
   * Asks the core what `typed` finds, one search at a time. Each search reads every Issue and PR on the
   * window's one connection to the database, so a fast typist's keys don't queue a search each ahead of
   * the Issue list's and the Detail pane's reads. A text typed while a search runs waits for it, and only
   * the latest of those goes next, unless it is the text under way. Each answer shows as it comes. An
   * arrow, so the palette can call it on its own; it settles once the searches it started have answered.
   *
   * A text that quick open looks for files for reads the diff of the PR on show, unless it has been read,
   * so a Ctrl+P for an Issue or a PR, or the command palette, reads none.
   */
  readonly find = async (typed: string): Promise<void> => {
    if (looksForFiles(typed)) this.files?.load();
    if (this.#searching !== undefined) {
      this.#next = typed === this.#searching ? undefined : typed;
      return;
    }
    let text: string | undefined = typed;
    while (text !== undefined) {
      this.#searching = text;
      const answer = await ask(commands.quickOpen(this.#repository, text));
      this.answer =
        answer.status === "ok"
          ? { typed: text, found: answer.data }
          : { typed: text, problem: `Tsugiki can't search the Issues and PRs: ${answer.error.message}` };
      text = this.#next;
      this.#next = undefined;
    }
    this.#searching = undefined;
  };

  /** Drops the text waiting for the search under way, as the palette closes: nobody reads its answer. */
  stop(): void {
    this.#next = undefined;
  }
}
