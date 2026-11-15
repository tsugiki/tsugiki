// Quick open's search of the open Repository's Issues and PRs in the cache (#557). The core does the search
// and puts the best first, and the window holds its answer to the latest text typed.
import { commands, type TitledItem_Serialize } from "$lib/data/bindings";
import { ask } from "./answer";

/** An Issue or a PR that quick open lists, named here for the page, which the UI's layers keep from the
 * bindings (`ui/eslint/layers.ts`). */
export type QuickOpenItem = TitledItem_Serialize;

/** The core's answer to one text typed: what it found, or why it couldn't search. */
export type QuickOpenAnswer = { typed: string } & ({ found: QuickOpenItem[] } | { problem: string });

export class QuickOpenState {
  readonly #repository: string;
  /** The answer to the last search that came back. */
  answer = $state<QuickOpenAnswer>();
  /** The text of the search under way, if one is. */
  #searching: string | undefined;
  /** The latest text typed while a search was under way, which goes next. */
  #next: string | undefined;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /**
   * Asks the core what `typed` finds, one search at a time. Each search reads every Issue and PR on the
   * window's one connection to the database, so a fast typist's keys don't queue a search each ahead of
   * the Issue list's and the Detail pane's reads. A text typed while a search runs waits for it, and only
   * the latest of those goes next, unless it is the text under way. Each answer shows as it comes. An
   * arrow, so the palette can call it on its own; it settles once the searches it started have answered.
   */
  readonly find = async (typed: string): Promise<void> => {
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
