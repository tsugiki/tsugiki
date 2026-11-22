// The core's answer to one question, asked once, then again each time one of the core's events says the
// answer may have changed (ADR 0008). A Repository's state and its last sync are both read so.
import { type Answer, ask } from "./answer";

/** Starts listening to an event and calls `again` each time it says the answer may have changed. Resolves to
 * what stops the listening. */
export type Listen = (again: () => void) => Promise<() => void>;

export class Followed<T> {
  /** Undefined until the core answers, and while it can't. */
  state = $state<T>();
  readonly #question: () => Promise<Answer<T>>;
  readonly #listens: Listen[];
  #lastQuestion = 0;
  #stops: (() => void)[] = [];
  #stopped = false;

  constructor(question: () => Promise<Answer<T>>, listens: Listen[]) {
    this.#question = question;
    this.#listens = listens;
  }

  /** Listens before it asks, so it hears a change the core makes meanwhile. */
  async load() {
    const listening = await Promise.allSettled(this.#listens.map((listen) => listen(() => void this.#ask())));
    this.#stops = listening.flatMap((result) => (result.status === "fulfilled" ? [result.value] : []));
    // Without every listener the answer could go stale, so the window shows none.
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
    const answer = await ask(this.#question);
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion || this.#stopped) return;
    this.state = answer.status === "ok" ? answer.data : undefined;
  }
}
