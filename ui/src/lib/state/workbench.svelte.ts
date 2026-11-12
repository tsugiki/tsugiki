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
import { SvelteMap, SvelteSet } from "svelte/reactivity";
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

/** The lines of `item` the cursor stops on, in the order the list shows them. An Issue's tree shows only
 * while `unfolded` says it does. */
function linesOf(item: ListedItem, unfolded: (issue: number) => boolean): Line[] {
  if (item.kind === "issue") {
    const tree = unfolded(item.number) ? item.pull_requests : [];
    const rows = tree.flatMap((entry) => (entry.kind === "pr" ? [entry, ...entry.backports] : [entry]));
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
  /** The Issues unfolded in each view, by the view's key. */
  #unfolded = new SvelteMap<string, SvelteSet<number>>();
  /** The view whose items the list shows: `selected` once its answer has arrived. */
  #shown = $state<WorkbenchView>({ kind: "whole" });
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

  /** Asks for the view on show again, as after the list's order changed. */
  async refresh() {
    await this.#ask();
  }

  /** The name of the view on show, as the Sidebar gives it. A Filter the settings no longer hold goes by
   * its search. */
  get title(): string {
    const selected = this.selected;
    if (selected.kind === "whole") return "Workbench";
    if (selected.kind === "review_requests") return "Review requests";
    return this.filters.find((filter) => filter.query === selected.query)?.name ?? selected.query;
  }

  /** The line under the cursor, wherever the list now has it. */
  get underCursor(): Stop | undefined {
    return this.#stops().find((stop) => sameStop(this.cursor, stop));
  }

  /** Whether the list, or its quiet section, has the line `stop`, folded away or not. */
  lists(stop: Stop): boolean {
    return this.#allStops().some((line) => sameStop(line, stop));
  }

  /** Whether the list shows Issue `issue`'s PRs under it in the view on show. Each Issue starts folded, and
   * each view keeps its own while the Repository stays open (#593). */
  unfolded(issue: number): boolean {
    return this.#open()?.has(issue) ?? false;
  }

  /** Shows Issue `issue`'s PRs under it. */
  unfold(issue: number) {
    const open = this.#open();
    if (open) {
      open.add(issue);
    } else {
      this.#unfolded.set(this.#viewKey(), new SvelteSet([issue]));
    }
  }

  /** Hides Issue `issue`'s PRs. A cursor on one of them moves to the Issue. */
  fold(issue: number) {
    this.#open()?.delete(issue);
    if (this.cursor?.issue === issue) this.cursor = { number: issue, kind: "issue", issue: undefined };
  }

  /** Puts the cursor on line `stop`, unfolding the Issue it's under so the cursor has a row to stand on. */
  moveTo(stop: Stop) {
    if (stop.issue !== undefined) this.unfold(stop.issue);
    this.cursor = stop;
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

  /** Moves the cursor after a PR that a change put under an Issue, or took out from under one, and unfolds
   * the Issue over a PR under the cursor, as another view may have it folded. */
  #followCursor() {
    const cursor = this.cursor;
    if (cursor === undefined) return;
    const stops = this.#allStops();
    const line =
      stops.find((stop) => sameStop(cursor, stop)) ?? stops.find((stop) => stop.number === cursor.number);
    if (line) this.moveTo(line);
  }

  /** The key of the view whose items the list shows, which keeps its own unfolded Issues. Until the answer
   * for a newly selected view arrives, the list still shows the last one's items, folded as it had them. */
  #viewKey() {
    const shown = this.#shown;
    return shown.kind === "filter" ? `filter ${shown.query}` : shown.kind;
  }

  /** The Issues unfolded in the view on show, if it has unfolded any yet. */
  #open() {
    return this.#unfolded.get(this.#viewKey());
  }

  /** The title of each line by its number, built once for each answer, which the history reads (#586). Made
   * anew whenever the list changes, so it needn't be reactive itself. It names the PRs of folded Issues
   * too. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #titles = $derived(new Map(this.#lines(() => true).map((line) => [line.stop.number, line.title])));

  /** The title of line `number`, if the list, or its quiet section, has it. */
  titleOf(number: number): string | undefined {
    return this.#titles.get(number);
  }

  /** The list's lines, in its order, with the trees `unfolded` says show. */
  #lines(unfolded: (issue: number) => boolean) {
    return [...(this.items ?? []), ...this.quiet].flatMap((item) => linesOf(item, unfolded));
  }

  /** The lines the list shows, in its order. */
  #stops() {
    const open = this.#open();
    return this.#lines((issue) => open?.has(issue) ?? false).map((line) => line.stop);
  }

  /** Each line the list has, the PRs of folded Issues too. */
  #allStops() {
    return this.#lines(() => true).map((line) => line.stop);
  }

  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const view = this.selected;
    const answer = await hud
      .timed(() => commands.workbench(this.#repository, view))
      .catch((e: unknown) => ({
        status: "error" as const,
        error: { message: String(e) },
      }));
    // A newer question reads newer facts, so this drops an older answer that arrives late.
    if (question !== this.#lastQuestion) return;
    if (answer.status === "ok") {
      this.items = answer.data.items;
      this.quiet = answer.data.quiet;
      this.#shown = view;
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
