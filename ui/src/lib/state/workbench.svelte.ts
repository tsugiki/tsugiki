// One Repository's Workbench and its Filters: asked for when the window opens it, when the user selects a
// line of the Sidebar, and each time the core says its facts changed (ADR 0008).
import {
  commands,
  events,
  type BackportKind,
  type BackportRow_Serialize,
  type CiState,
  type FilterLine_Serialize,
  type IssueItem_Serialize,
  type PrState,
  type TreeEntry_Serialize,
  type WorkbenchItem_Serialize,
  type WorkbenchRow_Serialize,
  type WorkReason,
} from "$lib/data/bindings";

export type BackportRow = BackportRow_Serialize;
export type FilterLine = FilterLine_Serialize;
export type IssueItem = IssueItem_Serialize;
export type TreeEntry = TreeEntry_Serialize;
export type WorkbenchItem = WorkbenchItem_Serialize;
export type WorkbenchRow = WorkbenchRow_Serialize;
export type { BackportKind, CiState, PrState, WorkReason };

export class WorkbenchState {
  items = $state<WorkbenchItem[]>();
  count = $state(0);
  filters = $state<FilterLine[]>([]);
  /** The search of the Filter on show, or null for the whole Workbench. */
  selected = $state<string | null>(null);
  error = $state<string>();
  #repository: string;
  #lastQuestion = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;

  constructor(repository: string) {
    this.#repository = repository;
  }

  /** Listens before it asks, so it hears a change that lands in between. */
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
      // With no listener the items would go stale, and the next good answer would clear this error.
      this.error = String(e);
      return;
    }
    await this.#ask();
  }

  /** Puts a Filter on show, or the whole Workbench for null. */
  async select(query: string | null) {
    this.selected = query;
    await this.#ask();
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await commands.workbench(this.#repository, this.selected).catch((e: unknown) => ({
      status: "error" as const,
      error: { message: String(e) },
    }));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.items = answer.data.items;
      this.count = answer.data.count;
      this.filters = answer.data.filters;
      this.error = undefined;
    } else {
      this.error = answer.error.message;
    }
  }
}
