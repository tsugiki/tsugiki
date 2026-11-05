// One Repository's Workbench and its Filters: asked for when the window opens it, when the user selects a
// line of the Sidebar, and each time the core says its facts changed (ADR 0008).
import {
  commands,
  events,
  type Attention_Serialize,
  type BackportKind,
  type BackportRow_Serialize,
  type CiState,
  type FilterLine_Serialize,
  type IssueItem_Serialize,
  type ListedItem_Serialize,
  type PrState,
  type ReviewState,
  type TreeEntry_Serialize,
  type WorkbenchRow_Serialize,
  type WorkbenchView,
  type WorkReason,
  type Why,
} from "$lib/data/bindings";

export type Attention = Attention_Serialize;
export type BackportRow = BackportRow_Serialize;
export type FilterLine = FilterLine_Serialize;
export type IssueItem = IssueItem_Serialize;
export type ListedItem = ListedItem_Serialize;
export type TreeEntry = TreeEntry_Serialize;
export type WorkbenchRow = WorkbenchRow_Serialize;
export type { BackportKind, CiState, PrState, ReviewState, WorkbenchView, WorkReason, Why };

export class WorkbenchState {
  items = $state<ListedItem[]>();
  quiet = $state<ListedItem[]>([]);
  count = $state(0);
  reviewRequests = $state(0);
  filters = $state<FilterLine[]>([]);
  selected = $state<WorkbenchView>({ kind: "whole" });
  error = $state<string>();
  /** The number of the item the list's keys act on. A re-query leaves it in place (ADR 0008). */
  cursor = $state<number>();
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
      // The list can gain an item it doesn't hold yet, so any change in this Repository asks again, whichever
      // items it names.
      const stopListening = await events.changed.listen(({ payload }) => {
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

  async select(view: WorkbenchView) {
    this.selected = view;
    await this.#ask();
  }

  /** The item under the cursor, wherever the list now has it. */
  get underCursor() {
    return this.#listed().find((item) => item.number === this.cursor);
  }

  /** Moves the cursor down the list, or up it, stopping at each end. With none yet it starts at the top. */
  moveCursor(step: 1 | -1) {
    const numbers = this.#listed().map((item) => item.number);
    const at = this.cursor === undefined ? -1 : numbers.indexOf(this.cursor);
    const next = at === -1 ? 0 : Math.min(Math.max(at + step, 0), numbers.length - 1);
    this.cursor = numbers[next] ?? this.cursor;
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
  }

  #listed() {
    return [...(this.items ?? []), ...this.quiet];
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
      this.quiet = answer.data.quiet;
      this.count = answer.data.count;
      this.reviewRequests = answer.data.review_requests;
      this.filters = answer.data.filters;
      this.error = undefined;
    } else {
      this.error = answer.error.message;
    }
  }
}
