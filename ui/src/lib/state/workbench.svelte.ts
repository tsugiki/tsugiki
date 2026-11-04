// One Repository's Workbench: asked for when the window opens it, and again each time the core says its facts
// changed (ADR 0008).
import {
  commands,
  events,
  type CiState,
  type WorkbenchRow_Serialize,
  type WorkReason,
} from "$lib/data/bindings";

export type WorkbenchRow = WorkbenchRow_Serialize;
export type { CiState, WorkReason };

export class WorkbenchState {
  rows = $state<WorkbenchRow[]>();
  error = $state<string>();
  #repository: string;
  #lastQuestion = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /** Listens before it asks, so a change that lands in between isn't lost. */
  async load() {
    try {
      const stopListening = await events.workbenchChanged.listen(({ payload }) => {
        if (payload.repository === this.#repository) void this.#ask();
      });
      if (this.#stopped) {
        stopListening();
        return;
      }
      this.#stopListening = stopListening;
    } catch (e) {
      // Rows now would never change, and the next good answer would clear this.
      this.error = String(e);
      return;
    }
    await this.#ask();
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await commands.workbench(this.#repository).catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    // A newer question knows more, so an older answer that arrives late has nothing to add.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.rows = answer.data.rows;
      this.error = undefined;
    } else {
      this.error = answer.error.message;
    }
  }
}
