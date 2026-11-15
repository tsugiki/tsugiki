// Quick open's search of the open Repository's Issues and PRs in the cache (#557). The core does the search
// and puts the best first, and the window holds its answer to the latest text typed.
import { commands, type QuickOpenItem_Serialize } from "$lib/data/bindings";
import { ask } from "./answer";

/** An Issue or a PR that quick open lists. */
export type QuickOpenItem = QuickOpenItem_Serialize;

/** The core's answer to one text typed: what it found, or why it couldn't search. */
export type QuickOpenAnswer = { typed: string } & ({ found: QuickOpenItem[] } | { problem: string });

export class QuickOpenState {
  readonly #repository: string;
  /** The answer to the latest text typed that has come back. An older text's answer never replaces it. */
  answer = $state<QuickOpenAnswer>();
  /** How many searches this has asked for. */
  #asked = 0;
  /** Which search `answer` answers, counted as `#asked` counts them. */
  #shown = 0;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /** Asks the core what `typed` finds. While the user types on, each answer shows as it comes, unless a
   * newer text's answer came first. An arrow, so the palette can call it on its own. */
  readonly find = async (typed: string): Promise<void> => {
    this.#asked += 1;
    const asked = this.#asked;
    const answer = await ask(commands.quickOpen(this.#repository, typed));
    if (asked < this.#shown) return;
    this.#shown = asked;
    this.answer =
      answer.status === "ok"
        ? { typed, found: answer.data }
        : { typed, problem: `Tsugiki can't search the Issues and PRs: ${answer.error.message}` };
  };
}
