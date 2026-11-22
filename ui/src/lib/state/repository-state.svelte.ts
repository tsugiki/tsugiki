// A Repository's state (#398): why Tsugiki can't write there, if it can't, and whether GitHub can be reached.
// Each write control reads it. Asked for once, then again each time the core says the Repository changed,
// as it does when the token's mark on it changes, and each time the status changes, which can say Tsugiki
// went offline or came back (ADR 0008).
import { commands, events, type Connection, type ReadOnly, type RepositoryState } from "$lib/data/bindings";
import { ask } from "./answer";

export type { Connection, ReadOnly, RepositoryState };

export class RepositoryStateView {
  /** Undefined until the core answers, and while it can't. */
  state = $state<RepositoryState>();
  readonly #repository: string;
  #lastQuestion = 0;
  #stops: (() => void)[] = [];
  #stopped = false;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /** Listens before it asks, so it hears a change the core makes meanwhile. */
  async load() {
    const listening = await Promise.allSettled([
      events.changed.listen(({ payload }) => {
        if (payload.repository === this.#repository) void this.#ask();
      }),
      events.statusChanged.listen(() => void this.#ask()),
    ]);
    this.#stops = listening.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
    // Without both listeners the state could go stale, so the window shows none.
    if (this.#stopped || listening.some((result) => result.status === "rejected")) {
      this.stop();
      return;
    }
    await this.#ask();
  }

  stop() {
    this.#stopped = true;
    for (const stop of this.#stops) stop();
    this.#stops = [];
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await ask(() => commands.repositoryState(this.#repository));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion || this.#stopped) return;
    this.state = answer.status === "ok" ? answer.data : undefined;
  }
}

/** Why Tsugiki can't write to `repository`, as each reason says it. */
const READ_ONLY: Record<ReadOnly, (repository: string) => string> = {
  token: (repository) => `your token can't write to ${repository}`,
};

/** The line atop the Issue and PR views of `repository` while it is Read-only, such as "Read-only: your
 * token can't write to python/cpython". Nothing, while Tsugiki can write there, or before the core says. */
export function readOnlyWords(state: RepositoryState | undefined, repository: string): string | null {
  if (state === undefined || state.read_only.length === 0) return null;
  const reasons = state.read_only.map((reason) => READ_ONLY[reason](repository));
  return `Read-only: ${reasons.join(", and ")}`;
}
