// One Repository's Workbench and its Filters, and its Inbox (#822): asked for when the window opens it, when
// the user selects a line of the Sidebar, and each time the core says its facts changed (ADR 0008).
import {
  commands,
  events,
  type Acted_Serialize,
  type Asked,
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
  type Listing,
  type ListOrder,
  type MuteAuthor,
  type PrState,
  type SeenMark_Serialize,
  type Seen_Serialize,
  type ShowMuted,
  type TreeEntry_Serialize,
  type Truncated,
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
/** A listed item the cache holds, as against a placeholder a search found (#465). */
export type CachedItem = Exclude<ListedItem, { kind: "placeholder" }>;
/** An item's part in Mark Seen and Mark Unseen, as the core drew it. */
export type Seen = Seen_Serialize;
export type SeenMark = SeenMark_Serialize;
export type TreeEntry = TreeEntry_Serialize;
export type WorkbenchRow = WorkbenchRow_Serialize;
export type {
  BackportKind,
  CantCheck,
  CiState,
  Hidden,
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

/** Whether a Muted author wrote the Issue or PR of a row, as Mute author offers Unmute author on it alone
 * (#494). */
export const byMutedAuthor = (row: { mute_author: MuteAuthor }) => row.mute_author.kind === "unmute";

/** A line of the Sidebar whose items the list shows: a view of the Workbench, or the Inbox (#822). */
export type SidebarView = WorkbenchView | { kind: "inbox" };

/** How long a pause in typing the Inbox's search box sends it to GitHub (#465). */
export const SEARCH_PAUSE_MS = 1000;

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

/** The Issue that Pin or Unpin acts on, with whether the user pinned it (#488). */
export type Pin = { number: number; pinned: boolean };

/** Whether two stops are the same line. A PR that fixes two Issues has a line under each. */
export const sameStop = (a: Stop | undefined, b: Stop) => a?.number === b.number && a.issue === b.issue;

/** Each PR an Issue's tree holds, in the order the list shows them: each entry, then its Backports. */
export const treeRows = (entries: TreeEntry[]): WorkbenchRow[] =>
  entries.flatMap((entry) => (entry.kind === "pr" ? [entry, ...entry.backports] : [entry]));

/** A line of the list: where the cursor stops, and the title it shows. */
type Line = { stop: Stop; title: string };

/** The key of view `view`, which keeps its own unfolded Issues. */
const keyOf = (view: SidebarView) => (view.kind === "filter" ? `filter ${view.query}` : view.kind);

/** Whether two keys name the same item. */
const sameKey = (a: ItemKey, b: ItemKey) => a.kind === b.kind && a.number === b.number;

/** The item of `items` whose own line `stop` is, as against a PR or Backport in its tree. */
const ownerOf = (stop: Stop, items: ListedItem[]) =>
  stop.issue === undefined ? items.find((item) => item.number === stop.number) : undefined;

/** An action's pending answer (#1052): the item the user acted on, whether the action finishes it, the line
 * the cursor stood on, and the lines the list then showed after the item's own. */
type Pending = { key: ItemKey; settles: boolean; was: Stop; after: Stop[] };

/** The lines of `item` the cursor stops on, in the order the list shows them. An Issue's tree shows only
 * while `unfolded` says it does. */
function linesOf(item: ListedItem, unfolded: (issue: number) => boolean): Line[] {
  // A placeholder is a result GitHub found that the cache lacks, and has no line to act on yet (#465).
  if (item.kind === "placeholder") return [];
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

/** The cursor's move once the core answered a user action that finished the selected item (#1052): `was`,
 * the line the cursor stood on, and `to`, the line it moved to, or nothing as it left the list, with no other
 * line to go to. */
export type Landing = { was: Stop; to: Stop | undefined };

/** The hooks a Workbench calls on the page that shows it, each with a default that does nothing. */
export type WorkbenchHooks = {
  /** Runs each time a user action finishes the selected item, once the cursor has moved on from it, as
   * `landing` says. */
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
  /** The count of Pinned issues on the Workbench that Need attention (#489). */
  pinned = $state(0);
  filters = $state<FilterLine[]>([]);
  selected = $state<SidebarView>({ kind: "whole" });
  error = $state<string>();
  /** The reason the Inbox's last older page couldn't be read, until one is. */
  olderError = $state<string>();
  /** The reason the last change to a Filter's "On the Workbench" didn't save, if it didn't. */
  switchProblem = $state<string>();
  /** A change to a Filter's "On the Workbench" is saving. */
  switching = $state(false);
  /** The Inbox's "Show muted" (#825), as the last answer about the Inbox says the core holds it. */
  showMuted = $state<ShowMuted>("off");
  /** The text of the Inbox's search box (#464). */
  search = $state("");
  /** Whether the search the last answer read needs GitHub, so the Inbox lists no local rows (#464). */
  needsGitHub = $state(false);
  /** The current Inbox search's request in flight or its result (#1109). */
  searchProgress = $state<{ kind: "asking" } | { kind: "answered" } | { kind: "failed"; message: string }>();
  /** How many results the search's answer listed of how many it reaches, past GitHub's 1,000 (#465). */
  truncated = $state<Truncated | null>(null);
  /** The Filter the Inbox's search would save as, with the trimmed box text it was read for, as the last
   * answer about the Inbox says (#471). */
  #filterQuery = $state<{ search: string; query: string }>();
  /** The timer that sends a paused search to GitHub (#465). */
  #searchTimer: ReturnType<typeof setTimeout> | undefined;
  /** Each send or edit advances this number, including an edit back to an earlier query. */
  #searchRequest = 0;
  /** A change to "Show muted" is saving. */
  savingShowMuted = $state(false);
  /** The reason the core refused the last change to "Show muted", if it did. */
  showMutedProblem = $state<string>();
  /** The "On the Workbench" switch each Filter's search has, as the user last set it, while that waits to
   * save. */
  #wanted = new SvelteMap<string, boolean>();
  /** The line the list's keys act on, or none, as when a change took its item out of the list. A re-query
   * leaves it on its item, wherever that moved (ADR 0008). */
  #cursor = $state<Stop>();
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
  /** The Inbox's placeholder numbers the list asked the core to fill, each once (#466). A plain `Set`: no
   * render reads it, so changing it doesn't run the page's effect for the list's items again. */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #placeholdersAsked = new Set<number>();
  /** The list's order as the window holds it now, and the order the items on show were asked in. */
  #order: () => ListOrder;
  #shownOrder: ListOrder | undefined;
  #landed: (landing: Landing) => void;
  #changing: () => () => void;
  #repository: string;
  #lastQuestion = 0;
  /** Each user action's ask, while its answer is on its way (#1052). */
  #pending = new SvelteMap<number, Pending>();
  /** The number the window gives the next action (#1052). */
  #lastAsked = 0;
  #stopListening: (() => void) | undefined;
  #stopped = false;
  /** The click on a name that last asked for its search, which alone writes the box (#472). */
  #personAsked = 0;

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
    // A test can mock an event without a way to stop listening, so what comes back stays unknown here.
    const stops: unknown[] = [];
    try {
      // The list can gain an item it doesn't hold yet, so any change in this Repository asks again, whichever
      // items it names.
      stops.push(
        await events.changed.listen(({ payload }) => {
          if (payload.repository !== this.#repository) return;
          // Nothing waits on this question, so a page hook that throws while it goes on show says so here.
          this.#ask().catch((e: unknown) => {
            this.error = String(e);
          });
        }),
      );
      // The core answers each action it kept with the list it showed, and the cursor and the Detail pane move
      // by that answer alone (#1052).
      stops.push(await events.acted.listen(({ payload }) => this.#answered(payload)));
      const kept = stops.filter((stop): stop is () => void => typeof stop === "function");
      if (this.#stopped) {
        for (const stop of kept) stop();
        return;
      }
      this.#stopListening = () => {
        for (const stop of kept) stop();
      };
    } catch (e) {
      // With no listener the items would go stale, and the next good answer would clear this error.
      for (const stop of stops) if (typeof stop === "function") stop();
      this.error = String(e);
      return;
    }
    await this.#ask();
  }

  async select(view: SidebarView) {
    this.selected = view;
    // A change of view is neither a user action nor a core change (#1052): an answer to an action made in
    // the view left behind moves nothing.
    this.#pending.clear();
    // A paused search the user abandoned by leaving the Inbox sends nothing then. Entering the Inbox with
    // a text Tsugiki never sent sends it now, so the box's text and the list agree again (#465).
    this.#clearSearchTimer();
    if (view.kind === "inbox") {
      if (this.search.trim() !== "" && this.searchProgress === undefined) void this.sendSearch();
    } else {
      this.needsGitHub = false;
      this.truncated = null;
      this.#filterQuery = undefined;
    }
    await this.#ask();
  }

  /** Narrows the Inbox to `text`, which the cache reads (#464), and asks on each key. An empty box brings
   * the whole list back. The pages on show belong to the search before this one, so this starts the new
   * one at its first page. An action's answer reads the listing it sent, whose search is now stale, so this
   * drops it like a change of view does. */
  async setSearch(text: string) {
    if (text === this.search) return;
    this.#replaceSearch(text);
    this.#reach = undefined;
    this.#pending.clear();
    // A pause of about a second sends the whole query to GitHub (#465). A new text drops the timer of the
    // text before it, as its answer would change nothing once the box holds this one.
    this.#clearSearchTimer();
    if (text.trim() !== "") {
      this.#searchTimer = setTimeout(() => void this.sendSearch(), SEARCH_PAUSE_MS);
    }
    await this.#ask();
  }

  /** Sends the Inbox's search box text to GitHub, which answers which rows it finds and how many (#465). The
   * `changed` event reads the Inbox again once the answer lands. The request itself says why it failed. */
  async sendSearch() {
    this.#clearSearchTimer();
    const text = this.search.trim();
    if (text === "") return;
    const request = ++this.#searchRequest;
    this.searchProgress = { kind: "asking" };
    const message = await problemIn(() => commands.searchInbox(this.#repository, text));
    if (request !== this.#searchRequest) return;
    this.searchProgress = message === undefined ? { kind: "answered" } : { kind: "failed", message };
  }

  #replaceSearch(text: string) {
    if (text === this.search) return;
    this.search = text;
    this.#searchRequest += 1;
    this.searchProgress = undefined;
  }

  #clearSearchTimer() {
    if (this.#searchTimer !== undefined) clearTimeout(this.#searchTimer);
    this.#searchTimer = undefined;
  }

  /** Opens the Inbox on the typed search that lists what `login` authored (#472): the box holds the query the
   * core spells, and the Inbox lists it as it lists any typed search. Answers whether the click still owned
   * the box, so a click a newer one overtook, or one that lands after the window stopped, changes nothing. */
  async openPerson(login: string): Promise<boolean> {
    const asked = (this.#personAsked += 1);
    const spelled = await ask(commands.personQuery(login));
    if (!this.#ownsPerson(asked)) return false;
    if (spelled.status === "error") {
      this.error = spelled.error.message;
      return false;
    }
    // The resets the box's own typing makes, before the Inbox sends and asks for the query below.
    this.#replaceSearch(spelled.data);
    this.#reach = undefined;
    this.needsGitHub = false;
    this.truncated = null;
    await this.select({ kind: "inbox" });
    return true;
  }

  /** Whether the *`asked`*'th click on a name still owns the box: no newer click overtook it, and the window
   * is still up (#472). */
  #ownsPerson(asked: number): boolean {
    return asked === this.#personAsked && !this.#stopped;
  }

  /** The Save as Filter dialog's draft: the box's text as its name, and the query with Tsugiki's additions
   * spelled out once the answer for the text on show has arrived (#471). Nothing while that answer waits or
   * failed, as the text alone doesn't say what the Filter would search. */
  get searchDraft(): { name: string; query: string } | undefined {
    const name = this.search.trim();
    if (name === "" || this.#filterQuery?.search !== name) return undefined;
    return { name, query: this.#filterQuery.query };
  }

  /** Saves the Inbox's search as a Filter named `name` that searches `query`, opens it in place of the Inbox,
   * and clears the box (#471). Answers the core's reason when it refuses one, as a name the Repository's
   * Filters already use. The Filter's first search runs at once, and its view lists the items as they
   * land. */
  async saveFilter(name: string, query: string): Promise<string | undefined> {
    const problem = await problemIn(commands.saveFilter(this.#repository, name, query));
    if (problem !== undefined) return problem;
    this.#replaceSearch("");
    this.#clearSearchTimer();
    this.needsGitHub = false;
    this.truncated = null;
    await this.select({ kind: "filter", query });
    return undefined;
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

  /** Asks the core to fill the placeholders with `numbers` that the list shows (#466), each number once:
   * the session fetches them in batches of 50. A placeholder the user scrolls back over costs no request.
   * One the core couldn't handle is dropped, so a later scroll asks again for it. */
  async fillPlaceholders(numbers: number[]): Promise<void> {
    const fresh = numbers.filter((number) => {
      if (this.#placeholdersAsked.has(number)) return false;
      this.#placeholdersAsked.add(number);
      return true;
    });
    if (fresh.length === 0) return;
    const answer = await ask(commands.fillInboxPlaceholders(this.#repository, fresh));
    const handled = answer.status === "ok" ? answer.data : [];
    for (const number of fresh) {
      if (!handled.includes(number)) this.#placeholdersAsked.delete(number);
    }
  }

  /** Asks for the view on show again once the list's order has changed. Its lines then moved for the order
   * alone, so none of them lands the cursor. */
  async reorder() {
    // A change of order is neither a user action nor a core change (#1052), but an action already sent still
    // answers, and its answer still moves the cursor on.
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
    if (selected.kind === "pinned") return "Pinned issues";
    return this.filters.find((filter) => filter.query === selected.query)?.name ?? selected.query;
  }

  /** The line the list's keys act on, which `moveTo` and `moveCursor` move. */
  get cursor(): Stop | undefined {
    return this.#cursor;
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

  /** Puts the cursor on line `stop`, or on none. */
  #put(stop: Stop | undefined) {
    this.#cursor = stop;
  }

  /** The selected item: the item that holds the line under the cursor (#1052). */
  get #selected(): ListedItem | undefined {
    const cursor = this.cursor;
    return cursor && this.#itemOf(cursor, this.#everything);
  }

  /**
   * The ask that names a user action, for the item `key` the action acts on, which `settles` says whether it
   * finishes (#1052). It remembers where the cursor stands, so the answer moves it only while it is still on
   * the item or the list has already taken it, and the rows the list shows after the item's own.
   */
  #asked(key: ItemKey | undefined, settles: boolean): Asked {
    const id = this.#lastAsked + 1;
    this.#lastAsked = id;
    const cursor = this.cursor;
    const selected = this.#selected;
    if (key && cursor && selected && sameKey(selected.key, key)) {
      this.#pending.set(id, { key, settles, was: cursor, after: this.#rowsAfter(selected) });
    }
    return { id, listing: this.#listing() };
  }

  /** The list the window shows, as the core reads it back for the answer to an action (#1052). */
  #listing(): Listing {
    const shown = this.#shown;
    return shown.kind === "inbox"
      ? { kind: "inbox", page: this.#againPage(), search: this.search }
      : { kind: "workbench", view: shown };
  }

  /** The rows the list shows after the lines of the item `item`, which the cursor stands on (#1052). */
  #rowsAfter(item: ListedItem): Stop[] {
    const items = this.#shownItems;
    const open = (issue: number) => this.#open()?.has(issue) ?? false;
    const after = items.slice(items.indexOf(item) + 1);
    return this.#lines(after, open).map((line) => line.stop);
  }

  /**
   * Moves the cursor and the Detail pane by the core's answer to an action it kept (#1052): to the row after
   * the acted item, or to the last row when nothing comes after it, once the answer says the action finished
   * the item or took it out of the list. An action that keeps the item in the list moves nothing. The answer
   * moves only while the cursor still stands on the item, or the list has already taken it and the cursor
   * with it, and only for the Repository and view that showed then. `landed` then hears where the cursor
   * went.
   */
  #answered({ id, repository, page }: Acted_Serialize) {
    if (repository !== this.#repository) return;
    const pending = this.#pending.get(id);
    // The core answers the actions it kept in the order it took them, so a later answer says that an action
    // before it failed, and no answer will come for it.
    for (const earlier of this.#pending.keys()) {
      if (earlier < id) this.#pending.delete(earlier);
    }
    if (!pending) return;
    // The cursor may be off the list already, as a change that came first took its item out, or on another
    // line of the item, as that change moved it. Any other line means the user moved the cursor, and the
    // answer no longer applies.
    if (this.cursor !== undefined && !sameStop(this.cursor, pending.was)) {
      const onItem = this.#itemOf(this.cursor, this.#everything);
      if (!onItem || !sameKey(onItem.key, pending.key)) return;
    }
    const items =
      page.kind === "inbox"
        ? page.inbox.items
        : this.mutedShown
          ? [...page.workbench.items, ...page.workbench.muted]
          : page.workbench.items;
    const item = items.find((listed) => sameKey(listed.key, pending.key));
    if (item && !pending.settles) return;
    const open = (issue: number) => this.#open()?.has(issue) ?? false;
    const lines = this.#lines(items, open).map((line) => line.stop);
    // The first row after the item that the answer still lists, or the last row once none is left.
    const to = pending.after.find((stop) => lines.some((line) => sameStop(line, stop))) ?? lines.at(-1);
    this.#put(to);
    this.#landed({ was: pending.was, to });
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
    if (item === undefined) return undefined;
    if (item.kind === "issue") {
      return [item, ...treeRows(item.pull_requests)].find((row) => row.number === stop.number);
    }
    const rows = [item, ...(item.kind === "standalone_pr" ? item.backports : [])];
    return rows.find((row) => row.number === stop.number);
  }

  /** Mark Seen with `mark`, as the window drew it, on line `stop` if the list has it. Of an item the window
   * draws `archived`, it goes through Archive, which marks seen the same way and keeps the item Archived
   * (#490), in its place in the Archived view (#491). On the selected item's own line, while the item Needs
   * attention or has something to mark seen, it finishes the item, and the core's answer moves the cursor on
   * (#1052). On a PR in an Issue's tree it marks the PR alone, which keeps the item. Says what went wrong, if
   * anything did. */
  async markSeen(stop: Stop | undefined, mark: SeenMark, archived?: ItemKey) {
    const owner = stop && ownerOf(stop, this.#everything);
    const settles = Boolean(
      owner && owner.kind !== "placeholder" && (owner.attention !== null || owner.seen.state !== "seen"),
    );
    const asked = this.#asked(owner?.key, settles);
    const marking = archived
      ? commands.keepArchived(this.#repository, { key: archived, mark }, asked)
      : commands.markSeen(this.#repository, mark, asked);
    return problemIn(marking);
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

  /** The Issue that holds line `stop`, with whether the user pinned it (#488). Nothing on a Standalone PR's
   * line, as only Issues take a pin. */
  pinOf(stop: Stop): Pin | undefined {
    const item = this.#itemOf(stop, this.#everything);
    return item?.kind === "issue" ? { number: item.number, pinned: item.pinned } : undefined;
  }

  /** Mark Unseen on Issue or PR `number`. It unarchives `archived`, the item that holds it if the window
   * draws that Archived, as the flag on a PR in an Archived Issue's tree would otherwise leave the Issue
   * archived (#491), which finishes the item in the Archived view. Says what went wrong, if anything did. */
  async markUnseen(number: number, archived?: ItemKey) {
    const key = archived && this.#unarchiving(archived);
    const asked = this.#asked(key, key !== undefined);
    const unseen = { number, archived: archived ?? null };
    return problemIn(commands.markUnseen(this.#repository, unseen, asked));
  }

  /** What Archive or Mute on line `stop` acts on: the item that holds the line, with its tree's part in Mark
   * Seen. */
  hidingBy(stop: Stop): Hiding | undefined {
    const item = this.#itemOf(stop, this.#everything);
    if (item === undefined) return undefined;
    return { key: item.key, mark: item.seen.mark };
  }

  /** Archive on `archived`, as the window drew it (#490), which finishes the item, and the core's answer
   * moves the cursor on (#1052). A Filter's view keeps an Archived item in its place (#824), and the Review
   * requests view an item whose Review request stands (#490), while any other view takes it out. Says what
   * went wrong, if anything did. */
  async archive({ key, mark }: Hiding) {
    const asked = this.#asked(key, true);
    return problemIn(commands.archive(this.#repository, { key, mark }, asked));
  }

  /** Mute on the item `key` names, with `mark` as the window drew it (#492), which finishes the item, and
   * the core's answer moves the cursor on (#1052). Says what went wrong, if anything did. */
  async mute({ key, mark }: Hiding) {
    const asked = this.#asked(key, true);
    return problemIn(commands.mute(this.#repository, { key, mark }, asked));
  }

  /** Unmutes the item `key` names, which stays in the list. Says what went wrong, if anything did. */
  async unmute(key: ItemKey) {
    return problemIn(commands.unmute(this.#repository, key));
  }

  /** Mute author on `login`, the author of line `stop` if the list holds one (#494). Their items leave the
   * list, and Mute author finishes the item that holds the line when they opened it (#1052). A PR alone that
   * kept the item listed takes row 1 instead: the core's answer takes the item out and moves the cursor on
   * (#1089). Says what went wrong, if anything did. */
  async muteAuthor(login: string, stop: Stop | undefined) {
    const item = stop && this.#itemOf(stop, this.#everything);
    const theirs = item !== undefined && item.mute_author.kind === "mute" && item.mute_author.login === login;
    const asked = this.#asked(item?.key, theirs);
    return problemIn(commands.muteAuthor(this.#repository, login, asked));
  }

  /** Unmutes the author `login`, whose items come back. Says what went wrong, if anything did. */
  async unmuteAuthor(login: string) {
    return problemIn(commands.unmuteAuthor(this.#repository, login));
  }

  /** Pins Issue `number` (#488), which then stays on the Workbench, leaves the Inbox, and leaves Archived as
   * the pin unarchives it, which finishes it there (#1052). Says what went wrong, if anything did. */
  async pin(number: number) {
    const kind = this.#shown.kind;
    const out = kind === "inbox" || kind === "archived";
    const asked = this.#asked({ kind: "issue", number }, out);
    return problemIn(commands.pin(this.#repository, number, asked));
  }

  /** Unpins Issue `number` (#488), which finishes it where the unpin takes it out (#1052): the Pinned issues
   * (#489), the Workbench while nothing but the pin keeps it there, and, for a pin that closed while pinned,
   * which the unpin archives, the Workbench and the Stale reviews. Says what went wrong, if anything did. */
  async unpin(number: number) {
    const unpin = this.#everything.find((item) => item.kind === "issue" && item.number === number)?.unpin;
    const kind = this.#shown.kind;
    const out =
      kind === "pinned" ||
      (kind === "whole" && (unpin === "leaves" || unpin === "archives")) ||
      (kind === "stale_reviews" && unpin === "archives");
    const asked = this.#asked({ kind: "issue", number }, out);
    return problemIn(commands.unpin(this.#repository, number, asked));
  }

  /** Unarchives the item `key` names, which finishes it in the Archived view, as the item leaves it (#491).
   * Says what went wrong, if anything did. */
  async unarchive(key: ItemKey) {
    const leaving = this.#unarchiving(key);
    const asked = this.#asked(leaving, leaving !== undefined);
    return problemIn(commands.unarchive(this.#repository, key, asked));
  }

  /** `key`, of an item to unarchive, while the list shows the Archived view, which the item then leaves.
   * Any other view keeps listing it. */
  #unarchiving(key: ItemKey): ItemKey | undefined {
    return this.#shown.kind === "archived" ? key : undefined;
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

  /** Turns the Inbox's "Show muted" to `show` (#825), and asks again once the core has saved it, or says why
   * it couldn't. `showMuted` changes only with that answer, or a later one. */
  async switchShowMuted(show: ShowMuted) {
    this.savingShowMuted = true;
    this.showMutedProblem = await problemIn(commands.switchShowMuted(this.#repository, show));
    this.savingShowMuted = false;
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
    this.#searchRequest += 1;
    this.#pending.clear();
    this.#clearSearchTimer();
    this.#stopListening?.();
  }

  /**
   * Places the cursor once an answer put `items` on show, which hold the Muted items only while they show
   * (#824), where `same` says whether the same view showed in the same order until then. Nothing an answer
   * brings is the user's action, so it moves the cursor only with its item or off the list (#1052).
   *
   * The cursor stays on its line wherever the list moved it, into the open Muted items too, and follows a
   * PR that a change put under an Issue, or took out from under one, unfolding the Issue over a PR under the
   * cursor, as another view may have it folded. When a change took the line out of the list on show, into
   * the folded Muted items too, the cursor leaves the list: no line is under it, and the next ↓ goes to the
   * first line. In a newly selected view that doesn't list the line, it stays on that line.
   */
  #placeCursor(items: ListedItem[], same: boolean) {
    const cursor = this.cursor;
    if (cursor === undefined) return;
    // The lines of `items`, folded Issues' PRs too, so the cursor never opens the folded Muted items.
    const stops = this.#lines(items, () => true).map((line) => line.stop);
    const line =
      stops.find((stop) => sameStop(cursor, stop)) ?? stops.find((stop) => stop.number === cursor.number);
    if (line) this.moveTo(line);
    else if (same) this.#put(undefined);
  }

  /** The item of `items` that holds line `stop`, under a folded Issue or not. A placeholder holds no line,
   * as the cache knows nothing of it (#465). */
  #itemOf(stop: Stop, items: ListedItem[]): CachedItem | undefined {
    return items.find(
      (item): item is CachedItem =>
        item.kind !== "placeholder" && linesOf(item, () => true).some((line) => sameStop(line.stop, stop)),
    );
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
    const search = this.search.trim();
    const [answer, list] = await Promise.all([
      ask(hud.timed(() => commands.workbench(this.#repository, inbox ? { kind: "whole" } : view))),
      inbox ? this.#inboxPage(this.#againPage()) : undefined,
    ]);
    if (!this.#current(question)) return;
    if (answer.status === "ok") {
      this.count = answer.data.count;
      this.reviewRequests = answer.data.review_requests;
      this.staleReviews = answer.data.stale_reviews;
      this.pinned = answer.data.pinned;
      this.filters = answer.data.filters;
    }
    if (list?.status === "ok") {
      this.showMuted = list.data.show_muted;
      this.needsGitHub = list.data.needs_github;
      this.truncated = list.data.truncated ?? null;
      this.#filterQuery =
        list.data.filter_query == null ? undefined : { search, query: list.data.filter_query };
    } else if (list?.status === "error") {
      this.needsGitHub = false;
      this.truncated = null;
      this.#filterQuery = undefined;
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
    const same = keyOf(view) === this.#viewKey() && order === this.#shownOrder && this.items !== undefined;
    const changed = same ? this.#changing() : undefined;
    this.items = items;
    this.muted = muted;
    this.#reach = reach;
    this.#shown = view;
    this.#shownOrder = order;
    // A fold that empties closes, so the next item muted goes into it folded.
    if (muted.length === 0) this.#mutedOpen.delete(keyOf(view));
    this.#placeCursor(this.#shownItems, same);
    changed?.();
  }

  /** The page of the Inbox that reads its pages on show again, as one: through where they end, all of them
   * once they hold all the cache does, or the first while the list shows another view. */
  #againPage(): InboxPage_Deserialize {
    const reach = this.#inboxReach;
    if (reach === undefined) return { kind: "latest" };
    return reach === null ? { kind: "all" } : { kind: "through", until: reach };
  }

  /** Reads page `page` of the Inbox, timed as a view query, narrowed by the search box's forms (#464). */
  #inboxPage(page: InboxPage_Deserialize) {
    return ask(hud.timed(() => commands.inbox(this.#repository, page, this.search)));
  }
}
