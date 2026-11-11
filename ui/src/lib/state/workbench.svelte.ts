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
  type IssueState,
  type ListedItem_Serialize,
  type PrState,
  type ReviewState,
  type TreeEntry_Serialize,
  type WorkbenchRow_Serialize,
  type WorkbenchView,
  type WorkReason,
  type Why,
} from "$lib/data/bindings";
import { hud } from "./hud.svelte";

export type Attention = Attention_Serialize;
export type BackportRow = BackportRow_Serialize;
export type FilterLine = FilterLine_Serialize;
export type IssueItem = IssueItem_Serialize;
export type ListedItem = ListedItem_Serialize;
export type TreeEntry = TreeEntry_Serialize;
export type WorkbenchRow = WorkbenchRow_Serialize;
export type { BackportKind, CiState, IssueState, PrState, ReviewState, WorkbenchView, WorkReason, Why };

/** A line the cursor stops on: an item of the list, or a PR or Backport in its tree, with the Issue above. */
export type Stop = { number: number; kind: "issue" | "pull_request"; issue: number | undefined };

/** Whether two stops are the same line. A PR that fixes two Issues has a line under each. */
export const sameStop = (a: Stop | undefined, b: Stop) => a?.number === b.number && a.issue === b.issue;

/** A line of the list: where the cursor stops, and the title it shows. */
type Line = { stop: Stop; title: string };

/** The lines of `item` the cursor stops on, in the order the list shows them. */
function linesOf(item: ListedItem): Line[] {
  if (item.kind === "issue") {
    const rows = item.pull_requests.flatMap((entry) =>
      entry.kind === "pr" ? [entry, ...entry.backports] : [entry],
    );
    return [
      { stop: { number: item.number, kind: "issue", issue: undefined }, title: item.title },
      ...rows.map((row) => ({
        stop: { number: row.number, kind: "pull_request" as const, issue: item.number },
        title: row.title,
      })),
    ];
  }
  const backports = item.kind === "standalone_pr" ? item.backports : [];
  return [item, ...backports].map((row) => ({
    stop: { number: row.number, kind: "pull_request", issue: undefined },
    title: row.title,
  }));
}

export class WorkbenchState {
  items = $state<ListedItem[]>();
  quiet = $state<ListedItem[]>([]);
  count = $state(0);
  reviewRequests = $state(0);
  filters = $state<FilterLine[]>([]);
  selected = $state<WorkbenchView>({ kind: "whole" });
  error = $state<string>();
  /** The line the list's keys act on. A re-query leaves it in place (ADR 0008). */
  cursor = $state<Stop>();
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

  /** The line under the cursor, wherever the list now has it. */
  get underCursor(): Stop | undefined {
    return this.#stops().find((stop) => sameStop(this.cursor, stop));
  }

  /** Whether the list, or its quiet section, has the line `stop`. */
  lists(stop: Stop): boolean {
    return this.#stops().some((line) => sameStop(line, stop));
  }

  /** Moves the cursor down the list, or up it, stopping at each end. With none yet it starts at the top. */
  moveCursor(step: 1 | -1) {
    const stops = this.#stops();
    const at = stops.findIndex((stop) => sameStop(this.cursor, stop));
    const next = at === -1 ? 0 : Math.min(Math.max(at + step, 0), stops.length - 1);
    this.cursor = stops[next] ?? this.cursor;
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
  }

  /** Moves the cursor after a PR that a change put under an Issue, or took out from under one. */
  #followCursor() {
    const cursor = this.cursor;
    const stops = this.#stops();
    if (cursor === undefined || stops.some((stop) => sameStop(cursor, stop))) return;
    this.cursor = stops.find((stop) => stop.number === cursor.number) ?? cursor;
  }

  /** Line `number`'s number and title, as the history names it (#586), or its number alone when the list
   * doesn't have it. */
  headlineOf(number: number): string {
    const line = this.#lines().find((found) => found.stop.number === number);
    return line ? `#${String(number)} ${line.title}` : `#${String(number)}`;
  }

  #lines() {
    return [...(this.items ?? []), ...this.quiet].flatMap(linesOf);
  }

  #stops() {
    return this.#lines().map((line) => line.stop);
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const answer = await hud
      .timed(() => commands.workbench(this.#repository, this.selected))
      .catch((e: unknown) => ({
        status: "error" as const,
        error: { message: String(e) },
      }));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.items = answer.data.items;
      this.quiet = answer.data.quiet;
      this.#followCursor();
      this.count = answer.data.count;
      this.reviewRequests = answer.data.review_requests;
      this.filters = answer.data.filters;
      this.error = undefined;
    } else {
      this.error = answer.error.message;
    }
  }
}
