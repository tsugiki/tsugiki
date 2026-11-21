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
  type Hidden,
  type InboxCursor_Serialize,
  type InboxPage_Deserialize,
  type IssueItem_Serialize,
  type IssueState,
  type ItemKey,
  type ListedItem_Serialize,
  type ListOrder,
  type MuteAuthor,
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
import { ask, problemIn } from "./answer";
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
  ItemKey,
  MuteAuthor,
  PrState,
  WorkbenchView,
  WorkReason,
  Why,
  YourReview,
};

/** Whether `hidden` hides a Muted item, on its own (#492) or as a Muted author's (#494). */
export const isMuted = (hidden: Hidden | null | undefined) => hidden === "muted" || hidden === "muted_author";

/** A line of the Sidebar whose items the list shows: a view of the Workbench, or the Inbox (#822). */
export type SidebarView = WorkbenchView | { kind: "inbox" };

/** `items` with each item of `more` that `items` doesn't list already. A sync can save between two pages
 * before its change reaches the window, which puts an item on both, and the list keys its items by number. */
function joined(items: ListedItem[], more: ListedItem[]): ListedItem[] {
  const listed = new Set(items.map((item) => item.number));
  return [...items, ...more.filter((item) => !listed.has(item.number))];
}

/** What Archive and Mute act on: the key of an item, and its tree's part in Mark Seen as the window drew
 * it. */
export type Hiding = { key: ItemKey; mark: SeenMark };

/** A line the cursor stops on: an item of the list, or a PR or Backport in its tree, with the Issue above. */
export type Stop = { number: number; kind: "issue" | "pull_request"; issue: number | undefined };

/** Whether two stops are the same line. A PR that fixes two Issues has a line under each. */
export const sameStop = (a: Stop | undefined, b: Stop) => a?.number === b.number && a.issue === b.issue;

/** Each PR an Issue's tree holds, in the order the list shows them: each entry, then its Backports. */
export const treeRows = (entries: TreeEntry[]): WorkbenchRow[] =>
  entries.flatMap((entry) => (entry.kind === "pr" ? [entry, ...entry.backports] : [entry]));

/** A line of the list: where the cursor stops, and the title it shows. */
type Line = { stop: Stop; title: string };

/** The key of view `view`, which keeps its own unfolded Issues. */
const keyOf = (view: SidebarView) => (view.kind === "filter" ? `filter ${view.query}` : view.kind);

/** The parts of `item`'s row that a mark changes: hidden or not, why it Needs attention, and how seen it
 * is. Two answers that draw it the same can't tell whether a mark between them took effect (#982). */
const drawnOf = (item: ListedItem) => JSON.stringify([item.hidden, item.attention, item.seen.state]);

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

/** The cursor's move once the line under it lost its attention or left the list: whether it finished the
 * item for a mark that `finish` waited on, and where it went, as `landed` hears them. */
export type Landing = {
  /** Whether the cursor left the item that `finish` waited for it to leave (#982). */
  finished: boolean;
  /** Onto the line of the first item after it that the list still shows; onto the last line, of another
   * item, when none came after it; onto the last line, still the same item's, as nothing else came after
   * it and the item stayed; or nowhere, as the cursor stays where it was with the list empty. */
  to: { kind: "next"; stop: Stop } | { kind: "last" } | { kind: "kept" } | { kind: "empty" };
};

/** The hooks a Workbench calls on the page that shows it, each with a default that does nothing. */
export type WorkbenchHooks = {
  /** Runs each time the line under the cursor loses its attention or leaves the list, once the cursor has
   * moved on from it, as `landing` says. */
  landed?: (landing: Landing) => void;
  /** The list's order as the window holds it when each question is asked. By attention without it. */
  order?: () => ListOrder;
  /** Runs before a change's answer replaces the lines of the view on show, in the same order, while the page
   * still shows the old ones. What it gives back runs once the cursor is placed on the new ones (#796). */
  changing?: () => () => void;
};

export class WorkbenchState {
  items = $state<ListedItem[]>();
  /** The items the list folds at its end behind "Show N muted": in a Filter's view, the Muted items its
   * search found that nobody asks the user about (#824). */
  muted = $state<ListedItem[]>([]);
  count = $state(0);
  reviewRequests = $state(0);
  staleReviews = $state(0);
  /** How many items the Archived view holds (#491). */
  archived = $state(0);
  filters = $state<FilterLine[]>([]);
  selected = $state<SidebarView>({ kind: "whole" });
  error = $state<string>();
  /** The reason the Inbox's last older page couldn't be read, until one is. */
  olderError = $state<string>();
  /** The reason the last change to a Filter's "On the Workbench" didn't save, if it didn't. */
  switchProblem = $state<string>();
  /** A change to a Filter's "On the Workbench" is saving. */
  switching = $state(false);
  /** The "On the Workbench" switch each Filter's search has, as the user last set it, while that waits to
   * save. */
  #wanted = new SvelteMap<string, boolean>();
  /** The line the list's keys act on. A re-query leaves it in place (ADR 0008). */
  cursor = $state<Stop>();
  /** The numbers of the items the user archived or unarchived whose answer hasn't placed the cursor yet.
   * Only `#placeCursor` reads it, so it needn't be reactive. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #leaving = new Set<number>();
  /** The item `finish` waits for the cursor to leave, as the list drew it when the first mark on it was
   * made, how many marks on it wait for the core's reply, and whether one of them did what it was asked
   * (#982). Only the cursor's moves and `#placeCursor` read it, so it needn't be reactive. */
  #finish: { number: number; drawn: string; replies: number; done: boolean } | undefined;
  /** The Issues unfolded in each view, by the view's key. */
  #unfolded = new SvelteMap<string, SvelteSet<number>>();
  /** The keys of the views whose fold of Muted items is open. */
  #mutedOpen = new SvelteSet<string>();
  /** The view whose items the list shows: `selected` once its answer has arrived. */
  #shown = $state<SidebarView>({ kind: "whole" });
  /** Where the Inbox's pages the list read last end: the next older page's start, or null once they hold all
   * the cache does. Only the Inbox's reads write it, one at a time, and it counts only while the list shows
   * the Inbox. */
  #reach = $state<InboxCursor | null>();
  /** The Inbox's reads, each chained after the one asked before it. */
  #reading: Promise<void> = Promise.resolve();
  /** Whether the Inbox's next older page is asked for and not yet read, so it is asked for once. */
  #olderAsked = false;
  /** The list's order as the window holds it now, and the order the items on show were asked in. */
  #order: () => ListOrder;
  #shownOrder: ListOrder | undefined;
  #landed: (landing: Landing) => void;
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
        // Nothing waits on this question, so a page hook that throws while it goes on show says so here.
        this.#ask().catch((e: unknown) => {
          this.error = String(e);
        });
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

  /** The view whose items the list shows: `selected` once its answer has arrived. */
  get shown(): SidebarView {
    return this.#shown;
  }

  /** Where the Inbox's pages on show end, while the list shows them. */
  get #inboxReach(): InboxCursor | null | undefined {
    return this.#shown.kind === "inbox" ? this.#reach : undefined;
  }

  /** Whether the list shows the Inbox and the cache holds older items than its pages on show. */
  get olderToRead(): boolean {
    return this.#inboxReach != null;
  }

  /** Reads the Inbox's next older page onto the end of the list, once the reads of the Inbox asked before
   * it are done, so it starts where the last of them ended. A call while it waits or reads does nothing. */
  async readOlder() {
    if (this.#olderAsked || !this.olderToRead) return;
    this.#olderAsked = true;
    await this.#inOrder(async () => {
      const after = this.#inboxReach;
      if (after != null) {
        const answer = await this.#inboxPage({ kind: "older", after });
        // Another view's answer can land meanwhile, as it doesn't read the Inbox.
        const items = this.items;
        if (!this.#stopped && this.#shown.kind === "inbox" && items !== undefined) {
          if (answer.status === "ok") {
            this.items = joined(items, answer.data.items);
            this.#reach = answer.data.older;
            this.olderError = undefined;
          } else {
            this.olderError = answer.error.message;
          }
        }
      }
      this.#olderAsked = false;
    });
  }

  /** Runs `read` once each read of the Inbox asked before it is done. One read at a time then reads the
   * Inbox, from where the one before it left the list, and only they write `#reach`. A read that throws, as
   * a page's hook can while its answer goes on show, rejects for its caller alone, and the next read runs. */
  #inOrder(read: () => Promise<void>): Promise<void> {
    const run = this.#reading.then(read);
    this.#reading = run.catch(() => {});
    return run;
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
    if (selected.kind === "archived") return "Archived";
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
    if (this.cursor?.issue === issue) this.#put({ number: issue, kind: "issue", issue: undefined });
  }

  /** Whether the list shows the Muted items it folds at its end. Each view starts with them folded, and
   * keeps its own while the Repository stays open, as with its Issues. */
  get mutedShown(): boolean {
    return this.#mutedOpen.has(this.#viewKey());
  }

  /** Shows the Muted items folded at the list's end, or folds them again. A cursor on one of them then
   * moves to the last line the list still shows, or to none when it shows none. */
  toggleMuted() {
    const key = this.#viewKey();
    if (!this.#mutedOpen.delete(key)) {
      this.#mutedOpen.add(key);
      return;
    }
    const cursor = this.cursor;
    if (cursor && this.#itemOf(cursor, this.muted)) this.#put(this.#stops().at(-1));
  }

  /** Puts the cursor on line `stop`, unfolding the Issue it's under, and the Muted items when it's one of
   * theirs, so the cursor has a row to stand on. */
  moveTo(stop: Stop) {
    if (stop.issue !== undefined) this.unfold(stop.issue);
    if (!this.mutedShown && this.#itemOf(stop, this.muted)) this.#mutedOpen.add(this.#viewKey());
    this.#put(stop);
  }

  /** Puts the cursor on line `stop`, or on none. A cursor that leaves the item a finish waits on stops the
   * wait, as only the cursor leaving that item for the mark finishes it (#982). */
  #put(stop: Stop | undefined) {
    this.cursor = stop;
    const holder = stop && this.#itemOf(stop, this.#everything);
    if (holder?.number !== this.#finish?.number) this.#finish = undefined;
  }

  /**
   * Waits for `marking`, the user's own Mark Seen, Archive or Mute that takes item `number` out of place,
   * and says what went wrong, if anything did (#982). While the cursor is on a line of that item, `landed`
   * then hears that the cursor leaving it finished it, whichever answer shows the item gone: the core
   * replies before it writes, so no answer's time can tell. The wait stops when the cursor leaves the item
   * for anything else, when an answer shows the item changed but in place, as a Filter's view keeps an
   * Archived item (#824) or a Stale review keeps it waiting (#908), and when each mark that waits on the
   * item failed.
   */
  async finish(number: number, marking: Promise<string | undefined>) {
    const cursor = this.cursor;
    const holder = cursor && this.#itemOf(cursor, this.#everything);
    if (holder?.number !== number) return marking;
    if (this.#finish?.number !== number) {
      this.#finish = { number, drawn: drawnOf(holder), replies: 0, done: false };
    }
    const finish = this.#finish;
    finish.replies += 1;
    const problem = await marking;
    finish.replies -= 1;
    if (problem === undefined) finish.done = true;
    else if (finish.replies === 0 && !finish.done && this.#finish === finish) this.#finish = undefined;
    return problem;
  }

  /** The part in Mark Seen of line `stop`: an Issue's covers its tree, and a PR's the PR alone. */
  seenOf(stop: Stop): Seen | undefined {
    return this.#lineOf(stop)?.seen;
  }

  /** What Mute author offers on line `stop` for the author of its Issue or PR (#494). */
  muteAuthorOf(stop: Stop): MuteAuthor | undefined {
    return this.#lineOf(stop)?.mute_author;
  }

  /** What line `stop` draws: its item's own line, or a PR or Backport in the item's tree. */
  #lineOf(stop: Stop): Pick<WorkbenchRow, "seen" | "mute_author"> | undefined {
    const item = this.#itemOf(stop, this.#everything);
    if (item?.number === stop.number) return item;
    const rows =
      item?.kind === "issue"
        ? treeRows(item.pull_requests)
        : item?.kind === "standalone_pr"
          ? item.backports
          : [];
    return rows.find((row) => row.number === stop.number);
  }

  /** Mark Seen with `mark`, as the window drew it. Of an item the window draws `archived`, it goes through
   * Archive, which marks seen the same way and keeps the item Archived (#490), in its place in the Archived
   * view (#491). Says what went wrong, if anything did. */
  async markSeen(mark: SeenMark, archived?: ItemKey) {
    const marking = archived
      ? commands.keepArchived(this.#repository, archived, mark)
      : commands.markSeen(this.#repository, mark);
    return problemIn(marking);
  }

  /** Whether `stop` is an item's own line, and that item Needs attention, so that Mark Seen on it moves the
   * cursor on from the line. */
  waits(stop: Stop): boolean {
    return ownerOf(stop, this.#everything)?.attention != null;
  }

  /** The key of the item that holds line `stop`, while the list draws it `hidden`: Archived (#490) or Muted
   * (#492), or a Muted author's (#494). */
  hiddenHolding(stop: Stop, hidden: Hidden): ItemKey | undefined {
    const item = this.#itemOf(stop, this.#everything);
    return item?.hidden === hidden ? item.key : undefined;
  }

  /** Whether the list draws the item that holds line `stop` Muted, on its own (#492) or as a Muted author's
   * (#494). */
  mutedHolding(stop: Stop): boolean {
    return isMuted(this.#itemOf(stop, this.#everything)?.hidden);
  }

  /** Mark Unseen on Issue or PR `number`. It unarchives `archived`, the item that holds it if the window
   * draws that Archived, as the flag on a PR in an Archived Issue's tree would otherwise leave the Issue
   * archived (#491). Says what went wrong, if anything did. */
  async markUnseen(number: number, archived?: ItemKey) {
    const marking = commands.markUnseen(this.#repository, number, archived ?? null);
    return this.#leave(archived && this.#unarchiving(archived), marking);
  }

  /** What Archive or Mute on line `stop` acts on: the item that holds the line, with its tree's part in Mark
   * Seen. */
  hidingBy(stop: Stop): Hiding | undefined {
    const item = this.#itemOf(stop, this.#everything);
    return item && { key: item.key, mark: item.seen.mark };
  }

  /** Archive on `archived`, as the window drew it (#490). Once the item leaves the list, a cursor on any of
   * its lines lands on the item after it. Says what went wrong, if anything did. */
  async archive({ key, mark }: Hiding) {
    return this.#leave(key, commands.archive(this.#repository, key, mark));
  }

  /** Mute on the item `key` names, with `mark` as the window drew it (#492). Once the item leaves the list, a
   * cursor on any of its lines lands on the item after it. Says what went wrong, if anything did. */
  async mute({ key, mark }: Hiding) {
    return this.#leave(key, commands.mute(this.#repository, key, mark));
  }

  /** Unmutes the item `key` names, which stays in the list. Says what went wrong, if anything did. */
  async unmute(key: ItemKey) {
    return problemIn(commands.unmute(this.#repository, key));
  }

  /** Mute author on `login`, the author of line `stop` if the list holds one (#494). Their items leave the
   * list, and once the item that holds the line leaves, as it does when they opened it, a cursor on any of
   * its lines lands on the item after it. Says what went wrong, if anything did. */
  async muteAuthor(login: string, stop: Stop | undefined) {
    const item = stop && this.#itemOf(stop, this.#everything);
    const theirs = item?.mute_author.kind === "mute" && item.mute_author.login === login;
    return this.#leave(theirs ? item.key : undefined, commands.muteAuthor(this.#repository, login));
  }

  /** Unmutes the author `login`, whose items come back. Says what went wrong, if anything did. */
  async unmuteAuthor(login: string) {
    return problemIn(commands.unmuteAuthor(this.#repository, login));
  }

  /** Unarchives the item `key` names. Once the item leaves the list, as it leaves the Archived view (#491),
   * a cursor on any of its lines lands on the item after it. Says what went wrong, if anything did. */
  async unarchive(key: ItemKey) {
    return this.#leave(this.#unarchiving(key), commands.unarchive(this.#repository, key));
  }

  /** `key`, of an item to unarchive, while the list shows the Archived view, which the item then leaves.
   * Any other view keeps listing it. */
  #unarchiving(key: ItemKey): ItemKey | undefined {
    return this.#shown.kind === "archived" ? key : undefined;
  }

  /** Waits for `marking`, a mark that takes the item `key` names off the list, if it names one, which then
   * lands the cursor on the item after it. Says what went wrong, if anything did. */
  async #leave(key: ItemKey | undefined, marking: Parameters<typeof problemIn>[0]) {
    if (key) this.#leaving.add(key.number);
    const problem = await problemIn(marking);
    if (key && problem !== undefined) this.#leaving.delete(key.number);
    return problem;
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
      const problem = await problemIn(
        commands.putFilterOnWorkbench({ repository: this.#repository, query, on }),
      );
      // An answer to a question asked before the save landed reads the settings as they were, so this has
      // `#ask` drop it, and the next question reads them as saved.
      this.#lastQuestion += 1;
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
    this.#put(stops[next] ?? this.cursor);
  }

  stop() {
    this.#stopped = true;
    this.#stopListening?.();
  }

  /**
   * Places the cursor once an answer put `items` on show, where `before` is what the same view showed in the
   * same order until then, if it did. Neither holds the Muted items while they're folded (#824), and
   * `inPlace` never does.
   *
   * When the line under the cursor, an item's own, Needed attention in `before`, and in `items` no longer
   * does or has left the list, the cursor lands on the first item after it in `before` that `items` still
   * lists, or else on the last line, and `landed` runs. So it does when the user archived the item that
   * holds the cursor's line, or unarchived it in the Archived view, on any of its lines, and no line of
   * `items` names that line's Issue or PR any more (#490, #491). So it does too when the user muted it and
   * it went into a Filter's Muted items, or a change put it into them while they're folded (#824). With
   * nothing listed it stays where it was, and `landed` runs too, as the Detail pane may wait for the item to
   * go (#982).
   *
   * Otherwise the cursor stays on its line, which may have moved, and follows a PR that a change put under
   * an Issue, or took out from under one, unfolding the Issue over a PR under the cursor, as another view
   * may have it folded.
   */
  #placeCursor(items: ListedItem[], inPlace: ListedItem[], before: ListedItem[] | undefined) {
    // An archive waits for the answer that takes its item off the list, as one that a sync asked for can
    // land before the archive's own, with the item still listed. An item the list doesn't hold waits no
    // more, and neither does one it holds marked hidden outside the Archived view, as a Filter's view keeps
    // an Archived or Muted item (#824).
    const leaving = [...this.#leaving];
    for (const number of leaving) {
      const listed = this.#everything.find((item) => item.number === number);
      const kept = listed?.hidden != null && this.#shown.kind !== "archived";
      if (!listed || kept) this.#leaving.delete(number);
    }
    const cursor = this.cursor;
    if (cursor === undefined) return;
    const owner = before && ownerOf(cursor, before);
    const quiet = owner?.attention && !items.find((item) => item.number === owner.number)?.attention;
    const was = before && this.#itemOf(cursor, before);
    // The list holds `items` by now.
    const namedIn = (list: ListedItem[]) =>
      this.#lines(list, () => true).some((line) => line.stop.number === cursor.number);
    const folded = !this.mutedShown && namedIn(this.muted);
    const left = was && !namedIn(inPlace) && (leaving.includes(was.number) || folded);
    if (before && was && (quiet || left)) {
      // The first item after it that the list still shows, folded Muted items left out.
      const stops = this.#stops();
      const owns = (number: number) =>
        stops.find((stop) => stop.number === number && stop.issue === undefined);
      const after = before.slice(before.indexOf(was) + 1);
      const next = after.map((item) => owns(item.number)).find((stop) => stop !== undefined);
      const last = stops.at(-1);
      const finished = this.#finish?.number === was.number;
      this.#finish = undefined;
      this.cursor = next ?? last ?? cursor;
      this.#landed({ finished, to: this.#landing(next, last, was) });
      return;
    }
    // The lines of `items`, folded Issues' PRs too, so the cursor never opens the folded Muted items.
    const stops = this.#lines(items, () => true).map((line) => line.stop);
    const line =
      stops.find((stop) => sameStop(cursor, stop)) ?? stops.find((stop) => stop.number === cursor.number);
    if (line) this.moveTo(line);
    // An item the answer draws changed but in place shows what the mark did, which then didn't move it.
    const holder = line && this.#itemOf(line, this.#everything);
    if (this.#finish && (!holder || drawnOf(holder) !== this.#finish.drawn)) this.#finish = undefined;
  }

  /** The place a cursor that left a line of `was` went to: onto `next`, the line of the first item after it
   * that the list still shows, or else onto `last`, the list's last line, which may be `was`'s own still. */
  #landing(next: Stop | undefined, last: Stop | undefined, was: ListedItem): Landing["to"] {
    if (next) return { kind: "next", stop: next };
    if (!last) return { kind: "empty" };
    return this.#itemOf(last, this.#everything)?.number === was.number ? { kind: "kept" } : { kind: "last" };
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

  /** Each item the list has, in its order, its folded Muted items last. */
  get #everything(): ListedItem[] {
    return [...(this.items ?? []), ...this.muted];
  }

  /** The lines of `items`, in their order, with the trees `unfolded` says show. */
  #lines(items: ListedItem[], unfolded: (issue: number) => boolean) {
    return items.flatMap((item) => linesOf(item, unfolded));
  }

  /** Each line the list has, in its order, the PRs of folded Issues and the folded Muted items too. */
  #allLines() {
    return this.#lines(this.#everything, () => true);
  }

  /** The items the list shows, in its order: the folded Muted items only while they show. */
  get #shownItems(): ListedItem[] {
    return this.mutedShown ? this.#everything : (this.items ?? []);
  }

  /** The lines the list shows, in its order. */
  #stops() {
    const open = this.#open();
    return this.#lines(this.#shownItems, (issue) => open?.has(issue) ?? false).map((line) => line.stop);
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

  /** Asks for the view `selected` names. A question about the Inbox reads it, so it waits its turn among
   * the Inbox's reads. */
  async #ask() {
    this.#lastQuestion += 1;
    const question = this.#lastQuestion;
    const view = this.selected;
    const order = this.#order();
    if (view.kind === "inbox") {
      await this.#inOrder(() => this.#answer(question, view, order));
    } else {
      await this.#answer(question, view, order);
    }
  }

  /**
   * Asks question `question`, for `view` in `order`, and puts what it answers on show while it is still the
   * last. For the Inbox, the Sidebar's counts and Filters come from the Workbench, and the list from one page
   * of the Inbox that reaches as far down as the pages on show did: the first page while the list shows
   * another view. A question that a newer one followed while it waited its turn asks nothing.
   */
  async #answer(question: number, view: SidebarView, order: ListOrder) {
    if (!this.#current(question)) return;
    const inbox = view.kind === "inbox";
    const [answer, list] = await Promise.all([
      ask(hud.timed(() => commands.workbench(this.#repository, inbox ? { kind: "whole" } : view))),
      inbox ? this.#inboxPage(this.#againPage()) : undefined,
    ]);
    if (!this.#current(question)) return;
    if (answer.status === "ok") {
      this.count = answer.data.count;
      this.reviewRequests = answer.data.review_requests;
      this.staleReviews = answer.data.stale_reviews;
      this.archived = answer.data.archived;
      this.filters = answer.data.filters;
    }
    const listed = list ?? answer;
    if (listed.status === "ok") {
      const muted = list === undefined && answer.status === "ok" ? answer.data.muted : [];
      this.#show(
        view,
        order,
        { items: listed.data.items, muted },
        list?.status === "ok" ? list.data.older : null,
      );
      this.olderError = undefined;
    }
    const failed = [answer, listed].find((one) => one.status === "error");
    this.error = failed?.error.message;
  }

  /** Puts `items` on show, with `muted` folded at their end, the answer for `view` in `order`, with the
   * cursor placed on them. For the Inbox, `reach` says where its pages among them end. The page's hooks run
   * first and last, so one that throws leaves the items and where they end as they were, or both as
   * answered. */
  #show(
    view: SidebarView,
    order: ListOrder,
    { items, muted }: { items: ListedItem[]; muted: ListedItem[] },
    reach: InboxCursor | null,
  ) {
    const same = keyOf(view) === this.#viewKey() && order === this.#shownOrder;
    const before = same && this.items ? this.#shownItems : undefined;
    const changed = before ? this.#changing() : undefined;
    this.items = items;
    this.muted = muted;
    this.#reach = reach;
    this.#shown = view;
    this.#shownOrder = order;
    // A fold that empties closes, so the next item muted goes into it folded.
    if (muted.length === 0) this.#mutedOpen.delete(keyOf(view));
    this.#placeCursor(this.#shownItems, items, before);
    changed?.();
  }

  /** The page of the Inbox that reads its pages on show again, as one: through where they end, all of them
   * once they hold all the cache does, or the first while the list shows another view. */
  #againPage(): InboxPage_Deserialize {
    const reach = this.#inboxReach;
    if (reach === undefined) return { kind: "latest" };
    return reach === null ? { kind: "all" } : { kind: "through", until: reach };
  }

  /** Reads page `page` of the Inbox, timed as a view query. */
  #inboxPage(page: InboxPage_Deserialize) {
    return ask(hud.timed(() => commands.inbox(this.#repository, page)));
  }
}
