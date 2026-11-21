// The Detail pane while Tsugiki fetches an Issue or a PR the database lacks (#981). It shows `owner/repo #N`
// loading at once. The core fetches the item ahead of the sync and saves it, and the pane then opens it as
// it opens any stored item, whichever kind the number turned out to be. A fetch that brought nothing says
// why, and the pane offers Open in browser.
import { commands, type ItemUnfetched } from "$lib/data/bindings";
import { ask } from "./answer";

/** The kind of item a fetch found, which the database now holds. */
export type FetchedKind = "issue" | "pull_request";

/** Why a fetch brought nothing: the core's reason, or what refused the command itself. */
export type FetchFailure = { why: ItemUnfetched } | { problem: string };

export class UnstoredViewState {
  readonly repository: string;
  readonly number: number;
  /** Why the fetch brought nothing, once it has failed. Undefined while it runs, and after it found the
   * item. */
  failure = $state.raw<FetchFailure>();
  #stopped = false;

  constructor(repository: string, number: number) {
    this.repository = repository;
    this.number = number;
  }

  /** Fetches the item, and hands `onfetched` its kind once the database holds it, unless the view has
   * stopped by then, as when the user opened something else meanwhile. */
  async load(onfetched: (kind: FetchedKind) => void): Promise<void> {
    const answer = await ask(commands.fetchItem(this.repository, this.number));
    if (this.#stopped) return;
    if (answer.status === "error") {
      this.failure = { problem: answer.error.message };
    } else if (answer.data.kind === "unfetched") {
      this.failure = { why: answer.data.why };
    } else {
      onfetched(answer.data.kind);
    }
  }

  /** The Detail pane shows something else now: an answer still on its way opens nothing. */
  stop(): void {
    this.#stopped = true;
  }
}
