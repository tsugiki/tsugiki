// One Repository's Workbench and its Filters, and its Inbox (#822): asked for when the window opens it, when
// the user selects a line of the Sidebar, and each time the core says its facts changed (ADR 0008).
import {
  commands,
  events,
  type Attention_Serialize,
  type BackportKind,
  type BackportRow_Serialize,
  type CantCheck,
  type CiState,
  type FilterLine_Serialize,
  type InboxCursor_Serialize,
  type InboxPage_Deserialize,
  type IssueItem_Serialize,
  type IssueState,
  type ListedItem_Serialize,
  type ListOrder,
  type PrState,
  type SeenMark_Serialize,
  type Seen_Serialize,
  type TreeEntry_Serialize,
  type WorkbenchRow_Serialize,
  type WorkbenchView,
  type WorkReason,
  type Why,
  type YourReview,
} from "$lib/data/bindings";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import { hud } from "./hud.svelte";

export type Attention = Attention_Serialize;
export type BackportRow = BackportRow_Serialize;
export type FilterLine = FilterLine_Serialize;
/** Where an item stands in the Inbox's order, as the core gives it. */
export type InboxCursor = InboxCursor_Serialize;
export type IssueItem = IssueItem_Serialize;
export type ListedItem = ListedItem_Serialize;
/** An item's part in Mark Seen and Mark Unseen, as the core drew it. */
export type Seen = Seen_Serialize;
export type SeenMark = SeenMark_Serialize;
export type TreeEntry = TreeEntry_Serialize;
export type WorkbenchRow = WorkbenchRow_Serialize;
export type {
  BackportKind,
  CantCheck,
  CiState,
  IssueState,
  PrState,
  WorkbenchView,
  WorkReason,
  Why,
  YourReview,
};

/** A line of the Sidebar whose items the list shows: a view of the Workbench, or the Inbox (#822). */
export type SidebarView = WorkbenchView | { kind: "inbox" };

/** Whether cursor `a` comes before cursor `b` in the Inbox's order: updated later, or at the same time with
 * a higher number. */
const comesBefore = (a: InboxCursor, b: InboxCursor) => {
  const [at, bt] = [Date.parse(a.updated_at), Date.parse(b.updated_at)];
  return at > bt || (at === bt && a.number > b.number);
};

/** `items` with each item of `more` that `items` doesn't list already. Pages read on either side of a change
 * can both hold an item, and the list keys its items by number. */
function joined(items: ListedItem[], more: ListedItem[]): ListedItem[] {
  const listed = new Set(items.map((item) => item.number));
  return [...items, ...more.filter((item) => !listed.has(item.number))];
}

/** The Inbox's pages read from the first, as one list, and where the next older page starts, or null once
 * the cache holds nothing older; or the reason a page couldn't be read. */
type InboxPages =
  { status: "ok"; items: ListedItem[]; older: InboxCursor | null } | { status: "error"; error: string };

/** A line the cursor stops on: an item of the list, or a PR or Backport in its tree, with the Issue above. */
export type Stop = { number: number; kind: "issue" | "pull_request"; issue: number | undefined };

/** Whether two stops are the same line. A PR that fixes two Issues has a line under each. */
export const sameStop = (a: Stop | undefined, b: Stop) => a?.number === b.number && a.issue === b.issue;

/** Each PR an Issue's tree holds, in the order the list shows them: each entry, then its Backports. */
export const treeRows = (entries: TreeEntry[]): WorkbenchRow[] =>
  entries.flatMap((entry) => (entry.kind === "pr" ? [entry, ...entry.backports] : [entry]));

/** The problem a command that answers nothing ran into, if it ran into one. */
const said = (answer: { status: "ok" } | { status: "error"; error: { message: string } }) =>
  answer.status === "error" ? answer.error.message : undefined;

/** The answer `asking` brings, with a command that threw read as one that answered with what it threw. */
const answered = <T>(asking: Promise<T>) =>
  asking.catch((e: unknown) => ({ status: "error" as const, error: { message: String(e) } }));

/** A line of the list: where the cursor stops, and the title it shows. */
type Line = { stop: Stop; title: string };

/** The key of view `view`, which keeps its own unfolded Issues. */
const keyOf = (view: SidebarView) => (view.kind === "filter" ? `filter ${view.query}` : view.kind);

/** The item of `items` whose own line `stop` is, as against a PR or Backport in its tree. Only that line
 * carries the item's attention. */
const ownerOf = (stop: Stop, items: ListedItem[]) =>
  stop.issue === undefined ? items.find((item) => item.number === stop.number) : undefined;

/** The lines of `item` the cursor stops on, in the order the list shows them. An Issue's tree shows only
 * while `unfolded` says it does. */
function linesOf(item: ListedItem, unfolded: (issue: number) => boolean): Line[] {
  if (item.kind === "issue") {
    const rows = unfolded(item.number) ? treeRows(item.pull_requests) : [];
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

/** The hooks a Workbench calls on the page that shows it, each with a default that does nothing. */
export type WorkbenchHooks = {
  /** Runs each time the cursor lands on the line after one that lost its attention. */
  landed?: () => void;
  /** The list's order as the window holds it when each question is asked. By attention without it. */
  order?: () => ListOrder;
  /** Runs before a change's answer replaces the lines of the view on show, in the same order, while the page
   * still shows the old ones. What it gives back runs once the cursor is placed on the new ones (#796). */
  changing?: () => () => void;
};

export class WorkbenchState {
  items = $state<ListedItem[]>();
  count = $state(0);
  reviewRequests = $state(0);
  staleReviews = $state(0);
  filters = $state<FilterLine[]>([]);
  selected = $state<SidebarView>({ kind: "whole" });
  error = $state<string>();
  /** The reason the last change to a Filter's "On the Workbench" didn't save, if it didn't. */
  switchProblem = $state<string>();
  /** A change to a Filter's "On the Workbench" is saving. */
  switching = $state(false);
  /** The "On the Workbench" switch each Filter's search has, as the user last set it, while that waits to
   * save. */
  #wanted = new SvelteMap<string, boolean>();
  /** The line the list's keys act on. A re-query leaves it in place (ADR 0008). */
  cursor = $state<Stop>();
  /** The Issues unfolded in each view, by the view's key. */
  #unfolded = new SvelteMap<string, SvelteSet<number>>();
  /** The view whose items the list shows: `selected` once its answer has arrived. */
  #shown = $state<SidebarView>({ kind: "whole" });
  /** Where the Inbox's next older page starts while the list shows the Inbox, or null once the cache holds
   * nothing older. Unset while the list shows a view of the Workbench. */
  #older = $state<InboxCursor | null>();
  /** Whether the Inbox's next older page is being read. */
  #readingOlder = false;
  /** How many answers have replaced the items on show, so a page read before the last of them is dropped. */
  #drawn = 0;
  /** The list's order as the window holds it now, and the order the items on show were asked in. */
  #order: () => ListOrder;
  #shownOrder: ListOrder | undefined;
  #landed: () => void;
  #changing: () => () => void;
  #repository: string;
  #lastQuestion = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;

  /** The Workbench of `repository`, with the hooks the page that shows it gives. */
  constructor(
    repository: string,
    { landed = () => {}, order = () => "attention", changing = () => () => {} }: WorkbenchHooks = {},
  ) {
    this.#repository = repository;
    this.#landed = landed;
    this.#order = order;
    this.#changing = changing;
  }

  /** Listens before it asks, so it hears a change that lands in between. */
  async load() {
    try {
      // The list can gain an item it doesn't hold yet, so any change in this Repository asks again, whichever
      // items it names.
      const stopListening = await events.changed.listen(({ payload }) => {
        if (payload.repository !== this.#repository) return;
        void this.#ask();
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

  async select(view: SidebarView) {
    this.selected = view;
    await this.#ask();
  }

  /** Asks for the view on show again, as after a change to the Repository's Conventions, which regroups its
   * PRs with no change to the store (ADR 0011). The Inbox reads its pages again from the first. */
  async refresh() {
    await this.#ask();
  }

  /** The view whose items the list shows: `selected` once its answer has arrived. */
  get shown(): SidebarView {
    return this.#shown;
  }

  /** Whether the list shows the Inbox and the cache holds older items than its pages on show. */
  get olderToRead(): boolean {
    return this.#older != null;
  }

  /** Reads the Inbox's next older page onto the end of the list, unless one is being read. A page read
   * before an answer replaced the items, as after a change, is dropped: that answer read the pages again. */
  async readOlder() {
    const [after, items] = [this.#older, this.items];
    if (after == null || items === undefined || this.#readingOlder) return;
    this.#readingOlder = true;
    const [question, drawn] = [this.#lastQuestion, this.#drawn];
    const answer = await this.#inboxPage({ kind: "older", after });
    this.#readingOlder = false;
    if (question !== this.#lastQuestion || drawn !== this.#drawn || this.#stopped) return;
    if (answer.status === "ok") {
      this.items = joined(items, answer.data.items);
      this.#older = answer.data.older;
      this.error = undefined;
    } else {
      this.error = answer.error.message;
    }
  }

  /** Asks for the view on show again once the list's order has changed. Its lines then moved for the order
   * alone, so none of them lands the cursor. */
  async reorder() {
    await this.#ask();
  }

  /** The name of the view on show, as the Sidebar gives it. A Filter the settings no longer hold goes by
   * its search. */
  get title(): string {
    const selected = this.selected;
    if (selected.kind === "whole") return "Workbench";
    if (selected.kind === "inbox") return "Inbox";
    if (selected.kind === "review_requests") return "Review requests";
    if (selected.kind === "stale_reviews") return "Stale reviews";
    return this.filters.find((filter) => filter.query === selected.query)?.name ?? selected.query;
  }

  /** The line under the cursor, wherever the list now has it. */
  get underCursor(): Stop | undefined {
    return this.#stops().find((stop) => sameStop(this.cursor, stop));
  }

  /** Whether the list has the line `stop`, under a folded Issue or not. */
  lists(stop: Stop): boolean {
    return this.#allStops().some((line) => sameStop(line, stop));
  }

  /** The first line the list has for the Issue or PR `number`, under a folded Issue too, or else a line of
   * its own, as for an item the view on show leaves out. */
  stopFor(number: number, kind: Stop["kind"]): Stop {
    return this.#allStops().find((stop) => stop.number === number) ?? { number, kind, issue: undefined };
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

  /** Puts the cursor on line `stop`, unfolding the Issue it's under, so the cursor has a row to stand on. */
  moveTo(stop: Stop) {
    if (stop.issue !== undefined) this.unfold(stop.issue);
    this.cursor = stop;
  }

  /** The part in Mark Seen of line `stop`: an Issue's covers its tree, and a PR's the PR alone. */
  seenOf(stop: Stop): Seen | undefined {
    const item = this.#itemOf(stop, this.items ?? []);
    if (item?.number === stop.number) return item.seen;
    const rows =
      item?.kind === "issue"
        ? treeRows(item.pull_requests)
        : item?.kind === "standalone_pr"
          ? item.backports
          : [];
    return rows.find((row) => row.number === stop.number)?.seen;
  }

  /** Mark Seen with `mark`, as the window drew it. Says what went wrong, if anything did. */
  async markSeen(mark: SeenMark) {
    return said(await commands.markSeen(this.#repository, mark));
  }

  /** Mark Unseen on item `number`. Says what went wrong, if anything did. */
  async markUnseen(number: number) {
    return said(await commands.markUnseen(this.#repository, number));
  }

  /** The Sidebar's line of the Filter whose view is selected, while the settings hold it. */
  get selectedFilter(): FilterLine | undefined {
    const selected = this.selected;
    return selected.kind === "filter"
      ? this.filters.find((filter) => filter.query === selected.query)
      : undefined;
  }

  /** Whether the Filter of line `filter` is on the Workbench, as its switch shows it: as the user last set
   * the switch while that waits to save, and as the settings hold it otherwise. */
  onWorkbench(filter: FilterLine): boolean {
    return this.#wanted.get(filter.query) ?? filter.on_workbench;
  }

  /** Puts the Filter that searches `query` on the Workbench, or takes it off (#819). One save goes at a
   * time, as two could land in either order, so a change made while one saves waits for it, and goes with
   * the switch as the user last set it. Once none is left to save, it asks again, as the Sidebar counts
   * such a Filter another way. Keeps the reason the first save of such a run failed in `switchProblem`,
   * until the next run. */
  async putOnWorkbench(query: string, on: boolean) {
    this.#wanted.set(query, on);
    if (this.switching) return;
    this.switching = true;
    // The first save of the run that fails says why, though a later one saves.
    let failed: string | undefined;
    for (let next = this.#wanted.entries().next(); !next.done; next = this.#wanted.entries().next()) {
      const [query, on] = next.value;
      const answer = await answered(
        commands.putFilterOnWorkbench({ repository: this.#repository, query, on }),
      );
      // An answer to a question asked before the save landed reads the settings as they were, so this has
      // `#ask` drop it, and the next question reads them as saved.
      this.#lastQuestion += 1;
      const problem = said(answer);
      failed ??= problem;
      this.switchProblem = failed;
      // The settings hold the switch as saved from now on, though the Sidebar's lines say so only once the
      // answer to the next question arrives.
      const saved = problem === undefined && this.filters.find((filter) => filter.query === query);
      if (saved) saved.on_workbench = on;
      if (this.#wanted.get(query) === on) this.#wanted.delete(query);
    }
    this.switching = false;
    await this.#ask();
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

  /**
   * Places the cursor once an answer put `items` on show, where `before` is what the same view showed in the
   * same order until then, if it did.
   *
   * When the line under the cursor, an item's own, Needed attention in `before`, and in `items` no longer
   * does or has left the list, the cursor lands on the first item after it in `before` that `items` still
   * lists, or else on the last line, and `landed` runs. With nothing listed it stays where it was.
   *
   * Otherwise the cursor stays on its line, which may have moved, and follows a PR that a change put under
   * an Issue, or took out from under one, unfolding the Issue over a PR under the cursor, as another view
   * may have it folded.
   */
  #placeCursor(items: ListedItem[], before: ListedItem[] | undefined) {
    const cursor = this.cursor;
    if (cursor === undefined) return;
    const was = before && ownerOf(cursor, before);
    if (before && was?.attention && !items.find((item) => item.number === was.number)?.attention) {
      const after = before.slice(before.indexOf(was) + 1).map((item) => item.number);
      const target = after.find((number) => items.some((item) => item.number === number));
      const stops = this.#stops();
      const landing =
        stops.find((stop) => stop.number === target && stop.issue === undefined) ?? stops.at(-1);
      if (landing === undefined) return;
      this.cursor = landing;
      this.#landed();
      return;
    }
    const stops = this.#allStops();
    const line =
      stops.find((stop) => sameStop(cursor, stop)) ?? stops.find((stop) => stop.number === cursor.number);
    if (line) this.moveTo(line);
  }

  /** The item of `items` that holds line `stop`, under a folded Issue or not. */
  #itemOf(stop: Stop, items: ListedItem[]): ListedItem | undefined {
    return items.find((item) => linesOf(item, () => true).some((line) => sameStop(line.stop, stop)));
  }

  /** The key of the view whose items the list shows, which keeps its own unfolded Issues. Until the answer
   * for a newly selected view arrives, the list still shows the last one's items, folded as it had them. */
  #viewKey() {
    return keyOf(this.#shown);
  }

  /** The Issues unfolded in the view on show, if it has unfolded any yet. */
  #open() {
    return this.#unfolded.get(this.#viewKey());
  }

  /** The title of each line by its number, built once for each answer, which the history reads (#586). Made
   * anew whenever the list changes, so it needn't be reactive itself. It names the PRs of folded Issues
   * too. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #titles = $derived(new Map(this.#allLines().map((line) => [line.stop.number, line.title])));

  /** The title of line `number`, if the list has it. */
  titleOf(number: number): string | undefined {
    return this.#titles.get(number);
  }

  /** The list's lines, in its order, with the trees `unfolded` says show. */
  #lines(unfolded: (issue: number) => boolean) {
    return (this.items ?? []).flatMap((item) => linesOf(item, unfolded));
  }

  /** Each line the list has, in its order, the PRs of folded Issues too. */
  #allLines() {
    return this.#lines(() => true);
  }

  /** The lines the list shows, in its order. */
  #stops() {
    const open = this.#open();
    return this.#lines((issue) => open?.has(issue) ?? false).map((line) => line.stop);
  }

  /** Each line the list has, the PRs of folded Issues too. */
  #allStops() {
    return this.#allLines().map((line) => line.stop);
  }

  /** Whether question `question` is still the last, for a Repository the window still shows. A newer
   * question reads newer facts, so an older answer that arrives late is dropped. */
  #current(question: number) {
    return question === this.#lastQuestion && !this.#stopped;
  }

  /** Asks for the view `selected` names. For the Inbox, the Sidebar's counts and Filters still come from the
   * Workbench, and the list's items from the Inbox's pages, read again from the first as far down as the
   * pages on show reached. */
  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const view = this.selected;
    const order = this.#order();
    const reach = keyOf(view) === this.#viewKey() ? this.#older : undefined;
    const workbenchView = view.kind === "inbox" ? ({ kind: "whole" } as const) : view;
    const [answer, inbox] = await Promise.all([
      answered(hud.timed(() => commands.workbench(this.#repository, workbenchView))),
      view.kind === "inbox" ? this.#readInbox(question, reach) : undefined,
    ]);
    if (!this.#current(question)) return;
    if (answer.status === "error") {
      this.error = answer.error.message;
      return;
    }
    if (inbox?.status === "error") {
      this.error = inbox.error;
      return;
    }
    const items = inbox ? inbox.items : answer.data.items;
    const same = keyOf(view) === this.#viewKey() && order === this.#shownOrder;
    const before = same ? this.items : undefined;
    const changed = before ? this.#changing() : undefined;
    this.items = items;
    this.#drawn += 1;
    this.#older = inbox?.older;
    this.#shown = view;
    this.#shownOrder = order;
    this.#placeCursor(items, before);
    changed?.();
    this.count = answer.data.count;
    this.reviewRequests = answer.data.review_requests;
    this.staleReviews = answer.data.stale_reviews;
    this.filters = answer.data.filters;
    this.error = undefined;
  }

  /** Reads page `page` of the Inbox, timed as a view query. */
  #inboxPage(page: InboxPage_Deserialize) {
    return answered(hud.timed(() => commands.inbox(this.#repository, page)));
  }

  /**
   * Reads the Inbox's pages for question `question`, from the first one down: the first alone while `reach`
   * is unset, and otherwise each page until one ends where the pages on show ended, at cursor `reach`, or
   * past it. Null `reach` reads every page the cache holds. Stops early once a newer question is asked.
   */
  async #readInbox(question: number, reach: InboxCursor | null | undefined): Promise<InboxPages> {
    let items: ListedItem[] = [];
    let page: InboxPage_Deserialize = { kind: "latest" };
    for (;;) {
      const answer = await this.#inboxPage(page);
      if (answer.status === "error") return { status: "error", error: answer.error.message };
      items = joined(items, answer.data.items);
      const older: InboxCursor | null = answer.data.older;
      const further = older !== null && reach !== undefined && (reach === null || comesBefore(older, reach));
      if (!further || !this.#current(question)) return { status: "ok", items, older };
      page = { kind: "older", after: older };
    }
  }
}
