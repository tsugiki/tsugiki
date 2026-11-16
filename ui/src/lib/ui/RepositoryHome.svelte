<script lang="ts">
  import { tick } from "svelte";
  import { openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import { lineOf, samePlace, type FileView, type HistoryPlace } from "$lib/state/file-view.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import { IssueViewState } from "$lib/state/issue-view.svelte";
  import {
    DETAIL_LEAST,
    LIST_LEAST,
    type ListOrder,
    PanesState,
    SIDEBAR_FOLD,
    SIDEBAR_ICONS,
    SIDEBAR_LEAST,
    SIDEBAR_MOST,
    SIDEBAR_WIDTH,
  } from "$lib/state/panes.svelte";
  import type { PeekLayout } from "$lib/state/peek";
  import { DiffViewChoice, type FilesMode, type PrFilesState } from "$lib/state/pr-files.svelte";
  import { PrViewState } from "$lib/state/pr-view.svelte";
  import type { ChangedFiles } from "$lib/state/quick-open.svelte";
  import type { TextSearchState } from "$lib/state/text-search.svelte";
  import { RAIL_ORDER, RAIL_TABS, type RailTabId } from "$lib/state/rail.svelte";
  import { Visits } from "$lib/state/visits.svelte";
  import { SvelteMap } from "svelte/reactivity";
  import {
    sameStop,
    WorkbenchState,
    type BackportRow,
    type ListedItem,
    type Seen,
    type Stop,
    type WorkbenchRow,
    type WorkbenchView,
  } from "$lib/state/workbench.svelte";
  import { keepOnScreen, scrollerIn } from "./focus";
  import HistoryBar from "./HistoryBar.svelte";
  import IssueDetail from "./IssueDetail.svelte";
  import type { ScreenKeys, View } from "./KeyList.svelte";
  import { type Action, actionOn, type Row } from "$lib/state/keymap.svelte";
  import {
    actionLabel,
    currentOs,
    keptByTextBox,
    keyName,
    keyTarget,
    type KeyTarget,
    type Pane,
    worksInTextBox,
  } from "./keys";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import type { PaletteAction, PaletteName } from "./Palette.svelte";
  import PaneEdge from "./PaneEdge.svelte";
  import PrDetail from "./PrDetail.svelte";
  import Sidebar from "./Sidebar.svelte";
  import SidebarIcons from "./SidebarIcons.svelte";
  import StateIcon from "./StateIcon.svelte";
  import {
    ago,
    detailWords,
    headline,
    offersMarkSeen,
    peopleOf,
    preview,
    summary,
    treeCount,
  } from "./workbench-row";

  const MINUTE = 60_000;
  /** How far ↑ or ↓ scrolls the Detail pane while the list is hidden, in CSS pixels, as a browser does. */
  const LINE = 40;
  /** The view of every PR's diff, unified or side by side, which `Ctrl+Alt+U` switches. */
  const diffView = new DiffViewChoice();
  /** How the user laid out the panes, which stays the same from one Repository to the next. */
  const panes = new PanesState();
  void panes.load();
  const EMPTY = {
    whole: "Nothing here yet. The first sync lists what waits on you as it finds it.",
    filter:
      "Nothing in this Filter yet. If a sync has run, check its query in settings.toml: GitHub finds nothing for a search it can't read.",
    review_requests: "Nobody has asked you for a review.",
    stale_reviews: "No Review of yours is stale.",
  };

  let {
    name,
    onpick,
    onsettings,
    keysActive = true,
    diffFiles = "all",
    diffPeek = "inline",
  }: {
    name: string;
    /** Opens the Repository picker, from the Repository's name atop the Sidebar. */
    onpick: () => void;
    /** Opens the Settings dialog, from ⚙ atop the Sidebar. */
    onsettings: () => void;
    keysActive?: boolean;
    /** Whether each PR's Files tab opens on all its files or one at a time, as `[diff] files` says. */
    diffFiles?: FilesMode;
    /** The place each Definition peek opens in, as `[diff] peek` says at the time. */
    diffPeek?: PeekLayout;
  } = $props();

  // A window can stay open between syncs, so "3h ago" moves on by itself.
  let now = $state(new Date());
  $effect(() => {
    const ticking = setInterval(() => {
      now = new Date();
    }, MINUTE);
    return () => {
      clearInterval(ticking);
    };
  });

  // Another Repository has its own Workbench, so a new name starts a new state and stops the last one.
  const workbench = $derived(new WorkbenchState(name, cursorLanded, () => panes.order, listChanging));
  /** The Issues, PRs and file views the Detail pane has shown, which Alt+← and Alt+→ go back and forward
   * through. Each Repository has its own. */
  const visits = $derived.by(() => {
    void name;
    return new Visits<HistoryPlace>(samePlace);
  });
  $effect(() => {
    // By the time this cleans up, `workbench` is already the next Repository's.
    const opened = workbench;
    void opened.load();
    return () => {
      opened.stop();
      swap(undefined);
      browserProblem = undefined;
      markProblem = undefined;
    };
  });

  // The first Workbench drawn ends the start-up the performance HUD times. Effects run once the rows are in
  // the page.
  $effect(() => {
    if (workbench.items !== undefined) void hud.markUsable();
  });

  /** The Issue or the PR in the Detail pane. */
  let detail = $state<IssueViewState | PrViewState>();
  /** The view of the Issue the PR on show came from, hidden until the user goes back to it (#191). */
  let kept: IssueViewState | undefined;
  let browserProblem = $state<string>();
  /** The reason the last Mark Seen or Mark Unseen didn't reach the core. */
  let markProblem = $state<string>();
  let list = $state<HTMLElement>();
  let home = $state<HTMLElement>();
  /** Whether the window holds the system's focus, which no CSS can ask. The panes read which of them holds
   * it with `:focus-within` (#602). */
  let windowFocused = $state(document.hasFocus());

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(name, item);
  };

  /** The title of each Issue and PR the Detail pane has shown, for the history's arrows and lists. Each
   * Repository has its own, as its numbers name other items. */
  const titles = $derived.by(() => {
    void name;
    return new SvelteMap<number, string>();
  });
  $effect(() => {
    const shown = detail?.view;
    if (shown) titles.set(shown.number, shown.title);
  });
  /** Item `number`'s headline, from its view once that has arrived, or else from its line in the list, or its
   * number alone when the list doesn't have it. */
  const headlineOf = (number: number) => {
    const title = titles.get(number) ?? workbench.titleOf(number);
    return title === undefined ? `#${String(number)}` : headline({ number, title });
  };
  /** A place's name in the history's lists: its item's headline, after the path for a file view. */
  const placeName = (place: HistoryPlace) =>
    place.kind === "file"
      ? `${place.view.place.path} · ${headlineOf(place.line.number)}`
      : headlineOf(place.number);
  /** How many items back the history's lists go, newest first. */
  const RECENT = 12;

  /** Goes `steps` places through the history, back for a negative number, as Alt+← and Alt+→ do. */
  function goThrough(steps: number) {
    const place = visits.go(steps);
    if (place) open(place, "through");
  }

  /** What hears of each file view the user opens over PR `line`'s Files tab, or closes with Back or Esc,
   * and records it in this Repository's history (#456): an opened view is a place of its own, and closing
   * it goes back to the PR's line. */
  function fileViewMoved(line: Stop) {
    const history = visits;
    return (view: FileView | undefined) => {
      history.visit(view ? { kind: "file", line, view } : line);
    };
  }

  /** Shows `view` over `pr`'s Files tab, or that PR with none, where it was scrolled. */
  function showFileView(pr: PrViewState, view: FileView | undefined) {
    if (view) pr.tab = "files";
    pr.files.showFileView(view);
  }

  /** The mouse's back and forward buttons go through the history, unless a dialog takes the keys. WebView2
   * would leave the page on them, so both presses are cancelled, and the release acts, as the engine does. */
  function mouseButton(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    if (e.type === "mouseup" && keysActive) goThrough(e.button === 3 ? -1 : 1);
  }

  /**
   * Shows the Issue or PR on line `stop` in the Detail pane. The PR already on show switches tabs instead,
   * and the Issue already on show reads GitHub again. Each counts as a visit, unless the history went
   * `"through"` the visits to it. Going `"back"` to the Issue the PR on show came from, or through the
   * visits to it, shows its view as it was and counts as no new opening. Both move the focus with the
   * cursor, so the row the user left doesn't look selected too, and Enter doesn't open it again. A pick
   * from the history's list can land on the item on show, which then only takes the cursor.
   *
   * The history can go through to a file view too (#456): it shows its PR, on the Files tab, with the view
   * over it, which takes the focus, and the cursor goes to the PR's row. Any other place shows its PR with no
   * file view, and so does a click on the row of the PR on show, which the history records as the PR's line.
   *
   * A `"pick"` from quick open opens as a click does, except that the cursor takes the focus, as the palette
   * has just handed it back, and the PR on show stays on its tab.
   */
  function open(place: HistoryPlace, how: "open" | "back" | "through" | "pick" = "open") {
    const stop = lineOf(place);
    const fileView = place.kind === "file" ? place.view : undefined;
    if (how !== "through") visits.visit(place);
    workbench.moveTo(stop);
    if (how !== "open" && fileView === undefined) focusCursor();
    if (stop.kind === "pull_request" && detail instanceof PrViewState && sameStop(detail.line, stop)) {
      if (how === "open") detail.switchTab();
      showFileView(detail, fileView);
      return;
    }
    if (stop.kind === "issue" && kept?.number === stop.number) {
      const resumed = kept;
      swap(resumed);
      if (how === "back" || how === "through") {
        resumed.show();
        return;
      }
      // Opening the Issue from the list starts at the top. Going back to it leaves the user where they were.
      resumed.scrollTop = 0;
    }
    const onShow =
      stop.kind === "issue" && detail instanceof IssueViewState && detail.number === stop.number
        ? detail
        : undefined;
    if (how === "through" && onShow) return;
    hud.issueOpened(stop.number);
    if (onShow) {
      // Opening leaves a view that was hidden nothing to catch up on when it shows.
      void onShow.open();
      onShow.show();
      return;
    }
    const next =
      stop.kind === "issue"
        ? new IssueViewState(name, stop.number)
        : new PrViewState(
            name,
            stop.number,
            stop.issue,
            panes,
            diffView,
            diffFiles,
            fileViewMoved(stop),
            () => diffPeek,
          );
    swap(next);
    void next.load();
    if (next instanceof PrViewState) showFileView(next, fileView);
  }

  /** Opens the Issue or PR `number` that quick open found (#557), on its line in the list if the list has
   * one, and moves the focus with the cursor, as going back does. An item the view on show leaves out has
   * no line to take the focus, which stays where it was, as when the history goes to such an item. */
  export function openFound(kind: Stop["kind"], number: number): void {
    // The PR on show keeps the line it opened from, so the pick finds it on show.
    const onShow = detail instanceof PrViewState && detail.number === number ? detail.line : undefined;
    open(onShow ?? workbench.stopFor(number, kind), "pick");
  }

  /** The files the PR on show changes, which quick open finds (#558), or nothing while no PR shows. Once
   * another view takes the Detail pane, that PR has no files to list, read or open. */
  export function changedFiles(): ChangedFiles | undefined {
    if (!(detail instanceof PrViewState)) return undefined;
    const pr = detail;
    return {
      load: () => {
        if (detail === pr) pr.loadFiles();
      },
      paths: () => (detail === pr ? pr.files.paths : []),
      open: (path) => {
        if (detail === pr) pr.openFile(path);
      },
    };
  }

  /** Puts `next` in the Detail pane. The Issue view a PR in its tree came from stays, hidden, while that PR
   * is on show, and every other view stops. A view that couldn't read its Issue stops too, so going back
   * opens the Issue afresh. */
  function swap(next: IssueViewState | PrViewState | undefined) {
    const from = next instanceof PrViewState ? next.issue : undefined;
    const keep = [detail, kept].find(
      (view): view is IssueViewState =>
        view instanceof IssueViewState && view.number === from && view.error === undefined,
    );
    for (const view of [detail, kept]) {
      if (view !== undefined && view !== keep && view !== next) view.stop();
    }
    keep?.hide();
    kept = keep;
    detail = next;
  }

  /** Leaves the PR view for the Issue the user opened it from, with the focus on the Issue's row. */
  function back(issue: number) {
    open({ number: issue, kind: "issue", issue: undefined }, "back");
  }

  /** The Issue or PR in the Detail pane, or else the line under the cursor while the list shows it. */
  function currentOnGitHub(): GitHubItem | undefined {
    const stop = detailLine() ?? (panes.hidden("list") ? undefined : workbench.underCursor);
    return stop && { kind: stop.kind, number: stop.number };
  }

  /** Ctrl+B: hides the Sidebar, or brings it back at its width, in full or as icons as it was. */
  function toggleSidebarByKey() {
    void reshapeSidebar(() => panes.toggle("sidebar"));
  }

  /** Shows the Sidebar as icons or in full, from «, the command palette, or the strip of icons (#587). */
  function showSidebar(icons: boolean) {
    void reshapeSidebar(() => panes.showSidebar(icons));
  }

  /** Changes how the Sidebar shows with `change`. Focus on a control of the Sidebar that goes, as « goes with
   * the full Sidebar, moves to the cursor's row, so it doesn't fall out of the window. */
  async function reshapeSidebar(change: () => Promise<void>) {
    const focused = document.activeElement;
    void change();
    await tick();
    if (focused?.isConnected === false) focusCursor();
  }

  /** The item Mark Seen and Mark Unseen act on: the item on show, or the line `stop` of the list. */
  type Marked = { stop: Stop | undefined; number: number; seen: Seen };

  /** The item the Detail pane shows, once its view has arrived. */
  function detailMarked(): Marked | undefined {
    const view = detail?.view;
    return view ? { stop: detailLine(), number: view.number, seen: view.seen } : undefined;
  }

  /** The pane a press on `target` went to, as `keyTarget` asks. A press that reaches no control, on the
   * page's body, goes to the list. */
  export function paneOf(target: EventTarget | null): Pane {
    const inList =
      target === document.body || (target instanceof HTMLElement && target.closest(".list") !== null);
    const inDetail = target instanceof Node && detailPane()?.contains(target) === true;
    return inList ? "list" : inDetail ? "detail" : null;
  }

  /** Whether a press `at` goes to the Detail pane: the pane holds the focus, or the list is hidden. ↑ ↓ then
   * scroll it, and `Q` marks its item. */
  const forDetail = (at: KeyTarget) => panes.hidden("list") || at.pane === "detail";

  /** The item `Q` and `Shift+Q` act on for a press `at`: the Detail pane's item while it holds the focus or
   * the list is hidden, and otherwise the line under the cursor. */
  function keyMarked(at: KeyTarget): Marked | undefined {
    if (forDetail(at)) return detailMarked();
    const stop = workbench.underCursor;
    const seen = stop && workbench.seenOf(stop);
    return stop && seen ? { stop, number: stop.number, seen } : undefined;
  }

  /** Says what went wrong with a mark that `marking` makes, unless the window has left the Repository it
   * was made in by the time it's done. */
  async function sayMarkProblem(marking: (opened: WorkbenchState) => Promise<string | undefined>) {
    const opened = workbench;
    const problem = await marking(opened);
    if (workbench === opened) markProblem = problem;
  }

  /** Mark Seen on `marked`, as the window drew it. Once the item no longer Needs attention, a cursor on its
   * row lands on the row that came after it, whether the item moved or kept its place, and `cursorLanded`
   * runs. */
  async function markSeen(marked: Marked | undefined) {
    if (marked) await sayMarkProblem((opened) => opened.markSeen(marked.seen.mark));
  }

  /** Takes the focus to the row the cursor landed on when a row of the list holds the focus, or the page
   * does, as it does once nothing else holds it. Focus anywhere else, such as in a text box or on the list's
   * header, stays put. */
  function cursorLanded() {
    const focused = document.activeElement;
    const onRow = focused instanceof HTMLElement && focused.matches("[data-item]") && list?.contains(focused);
    if (onRow || focused === document.body) focusCursor();
  }

  /** Before a change in the view on show: what brings the row of the Detail pane's line back on the screen
   * once the change is drawn, if the change moved it off, while the cursor is on it (#796). The row gets
   * back the focus it held, as moving a focused element in the page drops its focus. */
  function listChanging(): () => void {
    const row = detailRow();
    if (!row) return () => {};
    const keep = keepOnScreen(row.closest(".row") as Element, row.closest(".list") as Element);
    const focused = document.activeElement === row;
    return () =>
      void tick().then(() => {
        const moved = detailRow();
        if (!moved) return;
        keep(moved.closest(".row") as Element);
        if (focused) moved.focus({ preventScroll: true });
      });
  }

  /** The row under the cursor, while the list shows it and it is the Detail pane's line. */
  function detailRow(): HTMLElement | null {
    const line = detailLine();
    if (!line || !sameStop(workbench.cursor, line) || !list) return null;
    return list.querySelector<HTMLElement>(`[data-item="${lineName(line)}"]`);
  }

  async function markUnseen(marked: Marked | undefined) {
    if (marked) await sayMarkProblem((opened) => opened.markUnseen(marked.number));
  }

  /** Mark Seen or Mark Unseen on the item on show, whichever its header's button offers. */
  function markByButton() {
    const marked = detailMarked();
    if (marked && offersMarkSeen(marked.seen)) void markSeen(marked);
    else void markUnseen(marked);
  }

  /** The line of the list the Detail pane shows, if it shows one. */
  function detailLine(): Stop | undefined {
    if (detail instanceof IssueViewState) return { number: detail.number, kind: "issue", issue: undefined };
    return detail?.line;
  }

  /** Ctrl+Alt+B: hides the list, with the focus going to the Detail pane, or brings it back at its width.
   * The cursor and the focus then go to the row of what the Detail pane shows, or else stay on the
   * cursor's row. */
  async function toggleListByKey() {
    void panes.toggle("list");
    await tick();
    if (!panes.hidden("list")) cursorToDetail();
    focusCursor();
  }

  /** Moves the cursor to the row of what the Detail pane shows, if the list has that row. */
  function cursorToDetail() {
    const line = detailLine();
    if (line && workbench.lists(line)) workbench.moveTo(line);
  }

  /** "Reset Layout": both panes on show at their default widths. A list it brings back takes the cursor and
   * the focus as on `Ctrl+Alt+B`. */
  async function resetLayout() {
    const listHidden = panes.hidden("list");
    void panes.resetLayout();
    if (!listHidden) return;
    await tick();
    cursorToDetail();
    focusCursor();
  }

  /** `Ctrl+K Z`: hides the Sidebar and the list, with the focus going to the Detail pane, or brings back the
   * ones it hid. A list it brings back takes the cursor and the focus as on `Ctrl+Alt+B`, and the Sidebar
   * alone leaves the focus in the Detail pane. Focus already in the Detail pane stays where it is while the
   * list stays hidden. */
  async function toggleDetailOnly() {
    void panes.toggleDetailOnly();
    await tick();
    // A list on show has just come back, as the press hides it whenever it shows.
    if (!panes.hidden("list")) cursorToDetail();
    if (!panes.hidden("list") || detailPane()?.contains(document.activeElement) !== true) focusCursor();
  }

  /** Runs `action` at the end of its chord, press `e`, as its key of one press would, as `[keys]` can give
   * any action a chord or a key of one press (#549). */
  export function runChord(action: Action, e: KeyboardEvent): void {
    runKey(null, action, e);
  }

  /** Whether the Detail pane shows a PR's view, with the tabs the rail sits beside. The key and the palette
   * command wait for it, as the PR view's button does by coming with the tabs. */
  const showsPr = () => shownPr() !== undefined;

  /** The PR view on show, while it shows the tabs the rail sits beside. */
  const shownPr = () => (detail instanceof PrViewState && detail.view ? detail : undefined);

  /** A rail tab's key or palette command, as `Ctrl+K F` is Files': opens the rail beside the tabs of the PR
   * on show on `tab`, or closes it while it shows `tab` (#603, #727). The rail hands on the focus it holds as
   * it goes. The key and the palette offer it only while `showsPr` says so. */
  function toggleRail(tab: RailTabId) {
    panes.railState.toggle(tab);
  }

  /** `Ctrl+Shift+F` or "View: Search in the Rail", which open the rail on Search in `view`, with the focus in
   * its box, as in VS Code (#686). The box takes the text selected on one line, or else the word picked in
   * the Files tab on show. With Search on show it takes only a selection, and the rail stays open, so the
   * query the user typed stays unless they selected other text. Text the box holds already runs no query
   * again, which keeps the folds and the cursor. */
  function searchInRail(view: PrViewState) {
    const rail = panes.railState;
    const seed = seedText(view.search, rail.open && rail.tab === "search");
    if (seed !== undefined && !view.search.holds(seed)) view.search.seed(seed);
    view.search.wantsFocus = true;
    panes.railState.pick("search");
  }

  /** The text selected on one line, or else, unless `selectionOnly`, the word picked in the file view or
   * the diff of the Files tab on show, as `Ctrl+Shift+F` puts it in `search`'s box. While the box has the
   * focus, the selection is the box's own text, which goes nowhere. */
  function seedText(search: TextSearchState, selectionOnly: boolean): string | undefined {
    const selected = search.boxFocused ? "" : (window.getSelection()?.toString() ?? "");
    if (selected !== "" && !selected.includes("\n")) return selected;
    if (selectionOnly) return undefined;
    const tab = filesTab();
    return (tab?.fileView ? tab.fileView.code.pick : tab?.picked)?.word;
  }

  /** The Files tab of the PR on show, while it shows its diff. The file view covers the diff, so the diff's
   * keys and commands wait until Back (#453). */
  function shownFiles(): PrFilesState | undefined {
    const files = filesTab();
    return files?.fileView === undefined ? files : undefined;
  }

  /** The Files tab of the PR on show, while that tab shows, with or without the file view over it. */
  function filesTab(): PrFilesState | undefined {
    return detail instanceof PrViewState && detail.tab === "files" ? detail.files : undefined;
  }

  /**
   * The palette's own name for each of the screen's actions that has one (#555): a title that changes with
   * what the action does, with an id so the palette's history keeps one place for both titles, or the title
   * the palette gave the command before the key table. Undefined leaves the action out of the palette where
   * its key would still run: Unfold Unchanged Files waits for files unchanged since a Stale review to be
   * folded (#387). Each other action goes by the key table's title.
   */
  const PALETTE_NAMES: Partial<Record<Action, () => PaletteName | undefined>> = {
    toggle_sidebar: () => ({
      title: panes.hidden("sidebar") ? "Show Sidebar" : "Hide Sidebar",
      id: "toggle_sidebar",
    }),
    toggle_issue_list: () => ({
      title: panes.hidden("list") ? "Show Issue List" : "Hide Issue List",
      id: "toggle_issue_list",
    }),
    toggle_detail_only: () => ({
      title: panes.detailOnly ? "Show the Panes" : "Show Only the Detail Pane",
      id: "toggle_detail_only",
    }),
    ...Object.fromEntries(
      RAIL_ORDER.map((id): [Action, () => PaletteName] => [
        RAIL_TABS[id].action,
        () => ({ title: `View: ${RAIL_TABS[id].name} in the Rail` }),
      ]),
    ),
    toggle_files_mode: () => ({
      title: shownFiles()?.mode === "one" ? "View: All Files" : "View: One File at a Time",
      id: "toggle_files_mode",
    }),
    unfold_unchanged: () => {
      const files = shownFiles();
      return files !== undefined && files.foldedUnchanged > 0
        ? { title: "View: Unfold Unchanged Files" }
        : undefined;
    },
  };

  /**
   * The palette's row for `action`, whose title in the key table is `title`, for a press `at`, where the
   * focus was as the palette opened (#555). Undefined where the action isn't the screen's, or can't run
   * there. The row runs what the key runs, once the palette has handed the focus back, so the focus goes
   * where the key would put it. The palette types nothing, so a row runs where a text box would keep the key.
   */
  export function paletteRow(
    { action, title }: Pick<Row, "action" | "title">,
    at: KeyTarget,
  ): PaletteAction | undefined {
    const run = SCREEN_ACTIONS[action]?.(at);
    if (run === undefined) return undefined;
    const named = PALETTE_NAMES[action];
    const name = named ? named() : { title };
    return name && { ...name, action, run: () => run(false) };
  }

  /** The commands this screen adds to the palette that run no action, which follow the actions. */
  export function paletteCommands(): PaletteAction[] {
    return [
      panes.order === "attention"
        ? { title: "Sort by Last Update", id: "sort_list", run: () => void sortBy("last_update") }
        : { title: "Sort by Attention", id: "sort_list", run: () => void sortBy("attention") },
      { title: "View: Stale Reviews", run: () => showView({ kind: "stale_reviews" }) },
      // Each shape but the one on show, so a hidden Sidebar offers both.
      ...(panes.hidden("sidebar") || !panes.sidebarIcons
        ? [{ title: "View: Sidebar as Icons", run: () => showSidebar(true) }]
        : []),
      ...(panes.hidden("sidebar") || panes.sidebarIcons
        ? [{ title: "View: Full Sidebar", run: () => showSidebar(false) }]
        : []),
    ];
  }

  /** A click on an icon of the strip: shows `view` in the list, as a click on its line in the full Sidebar
   * does, and brings back the full Sidebar at its width, with one save for both. */
  function showViewInFull(view: WorkbenchView) {
    void workbench.select(view);
    void reshapeSidebar(() => panes.showFullSidebarAndList());
  }

  /** Shows `view` in the list, as a click on its line in the Sidebar does. */
  function showView(view: WorkbenchView) {
    void workbench.select(view);
    void panes.show("list");
  }

  /** The names the list's switch gives each order. */
  const ORDERS: Record<ListOrder, string> = { attention: "By attention ⇅", last_update: "By last update ⇅" };

  /** Orders the list by `order`: the core reads it in the order `machine.toml` keeps, so the list asks again
   * once that's saved. */
  async function sortBy(order: ListOrder) {
    await panes.setOrder(order);
    await workbench.reorder();
  }

  /** Names a line for the page, which can list a PR under two Issues. */
  const lineName = (stop: Stop | undefined) => `${String(stop?.issue ?? "")}#${String(stop?.number)}`;

  function moveCursor(step: 1 | -1) {
    workbench.moveCursor(step);
    focusCursor();
  }

  /** Whether a press `at` went to the list, or to no control at all, while the list shows. */
  const inList = (at: KeyTarget) => !panes.hidden("list") && at.pane === "list";

  /** → unfolds the Issue under the cursor, and ← folds it. ← on a PR in an Issue's tree moves the cursor to
   * the Issue, as in VS Code's trees (#593). A PR outside any Issue has nothing to fold. */
  function foldByKey(action: "unfold" | "fold") {
    const line = workbench.underCursor;
    if (line?.kind === "issue") {
      if (action === "unfold") workbench.unfold(line.number);
      else workbench.fold(line.number);
    } else if (line?.issue !== undefined && action === "fold") {
      workbench.moveTo({ number: line.issue, kind: "issue", issue: undefined });
      focusCursor();
    }
  }

  /** What an action runs, told whether a held key repeats it: the palette's run never does. */
  type Run = (held: boolean) => void;

  /** One of the screen's actions: what a press `at` runs, or undefined where the action can't run. The keys
   * and the palette run what it gives, and the `?` list asks it, so none of them can disagree. */
  type ScreenAction = (at: KeyTarget) => Run | undefined;

  /** Switches the tabs of the PR view on show, while one shows. */
  const tabSwitch: ScreenAction = () => {
    const view = detail instanceof PrViewState ? detail : undefined;
    return (
      view &&
      (() => {
        view.switchTab();
      })
    );
  };

  /** Runs `step` on the Files tab's diff while it shows. The file view covers the diff, so the diff's keys
   * wait until Back (#453). */
  const onDiff =
    (step: (files: PrFilesState) => void): ScreenAction =>
    () => {
      const files = shownFiles();
      return (
        files &&
        (() => {
          step(files);
        })
      );
    };

  /** Runs `step` on the Files tab, with or without the file view over it, which looks up the word picked
   * in it (#457). A held key would ask the language server again and again. */
  const onTab =
    (step: (tab: PrFilesState) => Promise<void>): ScreenAction =>
    () => {
      const tab = filesTab();
      return (
        tab &&
        ((held) => {
          if (!held) void step(tab);
        })
      );
    };

  /** Marks the item `Q` and `Shift+Q` act on, while there is one, as `keyMarked` says. It reads the item as
   * it runs, so a sync that lands while the palette is open marks what the window shows by then. A held key
   * would ask the core again and again. */
  const marking =
    (mark: (marked: Marked | undefined) => Promise<void>): ScreenAction =>
    (at) =>
      keyMarked(at) &&
      ((held) => {
        if (!held) void mark(keyMarked(at));
      });

  /** → and ← in the list. Elsewhere the bare arrows are a pane edge's, or scroll a diff or a code block
   * sideways. */
  const folding =
    (action: "unfold" | "fold"): ScreenAction =>
    (at) =>
      inList(at)
        ? () => {
            foldByKey(action);
          }
        : undefined;

  /** The screen's actions, each with what its key runs and when. Those that run in a text box are in
   * `worksInTextBox`. A held key on Ctrl+B or Ctrl+Alt+B would flicker the pane and save the file each
   * time. */
  const SCREEN_ACTIONS: Partial<Record<Action, ScreenAction>> = {
    toggle_sidebar: () => (held) => {
      if (!held) toggleSidebarByKey();
    },
    open_on_github: () => {
      const current = currentOnGitHub();
      return current && (() => void onGitHub(current));
    },
    toggle_diff_view: onDiff(() => diffView.toggle()),
    find_in_diff: onDiff((files) => files.openSearch()),
    toggle_detail_only: () => () => void toggleDetailOnly(),
    toggle_files_mode: onDiff((files) => files.toggleMode()),
    unfold_unchanged: onDiff((files) => files.unfoldUnchanged()),
    // Each rail tab's key, as Ctrl+K F is Files', while a PR shows (#727).
    ...Object.fromEntries(
      RAIL_ORDER.map((id): [Action, ScreenAction] => [
        RAIL_TABS[id].action,
        () =>
          showsPr()
            ? () => {
                toggleRail(id);
              }
            : undefined,
      ]),
    ),
    // Search's key opens the rail on Search and focuses its box, and doesn't close the rail (#686).
    search_in_rail: () => {
      const view = shownPr();
      return (
        view &&
        (() => {
          searchInRail(view);
        })
      );
    },
    mark_seen: marking(markSeen),
    mark_unseen: marking(markUnseen),
    toggle_issue_list: () => (held) => {
      if (!held) void toggleListByKey();
    },
    unfold: folding("unfold"),
    fold: folding("fold"),
    next_tab: tabSwitch,
    previous_tab: tabSwitch,
    next_file: onDiff((files) => files.nextFile(1)),
    previous_file: onDiff((files) => files.nextFile(-1)),
    next_thread: onDiff((files) => files.nextThread(1)),
    previous_thread: onDiff((files) => files.nextThread(-1)),
    next_change: onDiff((files) => files.nextChange(1)),
    previous_change: onDiff((files) => files.nextChange(-1)),
    peek_definition: onTab((tab) => tab.peekDefinition()),
    go_to_definition: onTab((tab) => tab.goToDefinition()),
    go_back: () => () => {
      goThrough(-1);
    },
    go_forward: () => () => {
      goThrough(1);
    },
    // It has no key unless `[keys]` gives it one.
    reset_layout: () => () => void resetLayout(),
  };

  /** What `action`'s key runs for a press `at`, or undefined where it can't run, or isn't the screen's. */
  function runnable(action: Action, at: KeyTarget): Run | undefined {
    const screenAction = SCREEN_ACTIONS[action];
    if (screenAction === undefined || (at.textBox && !worksInTextBox(action))) return;
    return screenAction(at);
  }

  /** Which keys of the screen work for a press `at`, for the `?` list: the list's while the press goes to
   * the list, as `inList` says, and the diff's while the Files tab shows. Of the screen's actions, those
   * that would run; the page decides on its own. */
  export function keysHere(at: KeyTarget): ScreenKeys {
    const views: View[] = [];
    if (inList(at)) views.push("issue_list");
    if (filesTab()) views.push("diff");
    return {
      views,
      works: (action) => !(action in SCREEN_ACTIONS) || runnable(action, at) !== undefined,
    };
  }

  /** Focuses the row under the cursor, so it scrolls into view, and Enter presses its button. While the list
   * is hidden the Detail pane takes the focus. Either waits for the page to update: the Detail pane to show
   * what it's about to, or the row to be drawn under an Issue that has only now unfolded. */
  function focusCursor() {
    void tick().then(() => {
      if (panes.hidden("list")) detailPane()?.focus();
      else list?.querySelector<HTMLElement>(`[data-item="${lineName(workbench.cursor)}"]`)?.focus();
    });
  }

  const detailPane = () => home?.querySelector<HTMLElement>('[data-pane="detail"]');

  /** Scrolls the Detail pane by a line, or the diff on a PR's Files tab, or the file view over it, or the
   * Conversation beside the rail, each of which scrolls by itself. */
  function scrollDetail(step: 1 | -1) {
    const pane = detailPane();
    const scroller = scrollerIn(pane) ?? pane;
    scroller?.scrollBy({ top: step * LINE });
  }

  /**
   * The list's keys and the PR view's. A dialog on top keeps them, and a text box or a focused control
   * keeps its own. The window's guard has already stopped the browser for Ctrl+B, Ctrl+Shift+G, Ctrl+Alt+U
   * and Ctrl+F, which work in a text box too unless `[keys]` gives them a key that types there, and for
   * Ctrl+Alt+B outside one, as AltGr sends it to type.
   * ↑ and ↓ go where the focus line shows (#602): they scroll the Detail pane while it holds the focus or
   * the list is hidden, and move the list's cursor otherwise. With the list hidden, Enter has no row to open.
   */
  function onkeydown(e: KeyboardEvent) {
    if (!keysActive) return;
    const key = keyName(e, currentOs());
    runKey(key, actionOn(key), e);
  }

  /** Runs what press `e` asks: the press `key`, null for a chord's, and its `action`. */
  function runKey(key: string | null, action: Action | undefined, e: KeyboardEvent) {
    const files = shownFiles();
    const tab = filesTab();
    // A key that types does so in a text box, whatever its action: `[keys]` may give Hide or Show the Sidebar
    // a plain key.
    if (key !== null && keptByTextBox(key, e.target)) return;
    const at = keyTarget(e.target, paneOf);
    // An action's key comes before the keys below only because none of them can be one: the core refuses a
    // line of `[keys]` that puts an action on Enter, Esc, ↑ or ↓ without Ctrl or Alt. Were it to allow
    // `go_back = "escape"`, Escape would stop closing the peek, the search and the file view. A key that runs
    // an action does nothing else: the bare ← and → that fold the list don't scroll it sideways too, and the
    // guard or the chord's wait has already stopped the browser for the rest.
    const run = action && runnable(action, at);
    if (run) {
      e.preventDefault();
      run(e.repeat);
    } else if (at.textBox) {
      return;
    } else if (key === "enter" && e.repeat) {
      // Each opening asks GitHub, and a held key isn't more than one.
      e.preventDefault();
    } else if ((key === "arrowdown" || key === "arrowup") && forDetail(at)) {
      e.preventDefault();
      scrollDetail(key === "arrowdown" ? 1 : -1);
    } else if (key === "arrowdown" || key === "arrowup") {
      e.preventDefault();
      moveCursor(key === "arrowdown" ? 1 : -1);
    } else if (
      key === "enter" &&
      e.target === document.body &&
      workbench.underCursor &&
      !panes.hidden("list")
    ) {
      e.preventDefault();
      open(workbench.underCursor);
    } else if (key === "escape" && (tab?.peek || tab?.going)) {
      // Escape closes the Definition peek first, or stops Go to definition, then the search, as in VS Code.
      // The search box keeps its own Escape, which closes the search. The peek can be in the file view, which
      // the next Escape closes (#457).
      e.preventDefault();
      tab.closePeek();
    } else if (key === "escape" && files?.searching) {
      e.preventDefault();
      files.closeSearch();
    } else if (key === "escape" && tab?.fileView) {
      // The file view goes back to the diff under it, and the next Escape goes on to the Issue (#456).
      e.preventDefault();
      tab.closeFileView();
    } else if (key === "escape" && detail instanceof PrViewState && detail.issue !== undefined) {
      // A PR opened from a row of its own has nothing under it, so it stays on show.
      e.preventDefault();
      back(detail.issue);
    }
  }

  // Joins a PR's facts. Svelte drops the spaces at the ends of an element's text, so it comes from here.
  const DOT = " · ";
</script>

<!-- An Issue's or a PR's number in the code font and its title, which name the button that opens it. One
     expression for each: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
{#snippet label(item: { number: number; title: string })}<code>{`#${String(item.number)}`}</code
  >{` ${item.title}`}{/snippet}

<!-- The icon sits beside the button, so the button's name stays the item's headline. The button's `::after`
     covers its row, so a click anywhere on the row opens the item. -->
{#snippet line(
  stop: Stop,
  item: { number: number; title: string },
  attention: boolean,
  icon: Octicon | undefined,
)}
  {#if icon}
    <StateIcon {icon} />
  {/if}
  <button
    class="link title"
    data-item={lineName(stop)}
    aria-current={sameStop(workbench.cursor, stop) ? "true" : undefined}
    onclick={() => {
      open(stop);
    }}
  >
    {#if attention}
      <strong>{@render label(item)}</strong>
    {:else}
      {@render label(item)}
    {/if}
  </button>
{/snippet}

<!-- A PR's or a Backport's row in an unfolded tree: its headline, and its facts under it. -->
{#snippet pullRequest(row: WorkbenchRow, issue: number | undefined)}
  <div class="row">
    <span class="headline">
      {@render line({ number: row.number, kind: "pull_request", issue }, row, false, prIcon(row))}
    </span>
    <small class="facts">{@render facts(row)}</small>
  </div>
{/snippet}

<!-- A PR's facts, with a Stale review in the danger colour and the files changed since the Review in its
     hover. -->
{#snippet facts(row: WorkbenchRow)}
  {#each detailWords(row) as { words, hover }, index (index)}{#if index > 0}{DOT}{/if}{#if hover}<span
        class="stale"
        title={hover}>{words}</span
      >{:else}{words}{/if}{/each}
{/snippet}

{#snippet backports(rows: BackportRow[], issue: number | undefined)}
  {#if rows.length > 0}
    <ul class="pull-requests">
      {#each rows as row (row.number)}
        <li>{@render pullRequest(row, issue)}</li>
      {/each}
    </ul>
  {/if}
{/snippet}

<!-- An item's three lines: the people, its headline with the size of its tree and its age, then why it Needs
     attention, or else a summary. -->
{#snippet listed(item: ListedItem)}
  {@const unfolded = item.kind === "issue" && workbench.unfolded(item.number)}
  <li class={["item", { attention: item.attention !== null }]}>
    <div class="row">
      <span class="people">{peopleOf(item)}</span>
      <span class="headline">
        {#if item.kind === "issue" && item.pull_requests.length > 0}
          <button
            class="fold"
            aria-label={`PRs of #${String(item.number)}`}
            aria-expanded={unfolded}
            onclick={() => {
              if (unfolded) workbench.fold(item.number);
              else workbench.unfold(item.number);
            }}>{unfolded ? "⌄" : "›"}</button
          >
        {:else}
          <!-- Keeps the icons and headlines of rows with nothing to fold in line with those that have. -->
          <span class="fold"></span>
        {/if}
        {@render line(
          { number: item.number, kind: item.kind === "issue" ? "issue" : "pull_request", issue: undefined },
          item,
          item.attention !== null,
          item.kind === "issue" ? issueIcon(item.state) : prIcon(item),
        )}
        {#if item.kind === "issue" && item.pull_requests.length > 0}
          <small class="count">{treeCount(item)}</small>
        {/if}
        <small class="age">{ago(item.updated_at, now)}</small>
      </span>
      <small class="facts"
        >{#if item.attention}{preview(item.attention)}{:else if item.kind !== "issue"}{@render facts(
            item,
          )}{:else}{summary(item)}{/if}</small
      >
    </div>
    {#if unfolded}
      <ul class="pull-requests">
        {#each item.pull_requests as entry (entry.number)}
          <li>
            {@render pullRequest(entry, item.number)}
            {#if entry.kind === "pr"}
              {@render backports(entry.backports, item.number)}
            {/if}
          </li>
        {/each}
      </ul>
    {:else if item.kind === "standalone_pr"}
      {@render backports(item.backports, undefined)}
    {/if}
  </li>
{/snippet}

{#snippet problems()}
  {#if workbench.error}
    <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
    <p role="alert">{`Tsugiki can't read this Workbench: ${workbench.error}`}</p>
  {/if}
  {#if browserProblem}
    <p role="alert">{browserProblem}</p>
  {/if}
  {#if markProblem}
    <p role="alert">{`Tsugiki couldn't mark this item: ${markProblem}`}</p>
  {/if}
  {#if panes.problem}
    <p role="alert">{panes.problem}</p>
  {/if}
{/snippet}

<svelte:window
  {onkeydown}
  onmousedown={mouseButton}
  onmouseup={mouseButton}
  onfocus={() => (windowFocused = true)}
  onblur={() => (windowFocused = false)}
/>

<!-- The bar atop the Detail pane, over item `shown`. -->
{#snippet history(shown: number)}
  <HistoryBar
    back={visits.behind(RECENT).map(placeName)}
    forward={visits.ahead(RECENT).map(placeName)}
    current={headlineOf(shown)}
    ongo={goThrough}
  />
{/snippet}

<!-- The Sidebar shows the name. This heading names the Repository to a screen reader whatever panes show. -->
<h1 class="hidden-name">{name}</h1>
<!-- The list says what went wrong, and while it's hidden a row above the panes does. -->
{#if panes.hidden("list")}
  {@render problems()}
{/if}
<!-- With another app in front, no pane shows the focus: the next key goes to that app. -->
<div
  class={["home", { "window-focused": windowFocused }]}
  style:--sidebar-width={`${String(panes.sidebar ?? SIDEBAR_WIDTH)}px`}
  style:--list-width={panes.list === null ? undefined : `${String(panes.list)}px`}
  bind:this={home}
>
  {#if !panes.hidden("sidebar")}
    {#if panes.sidebarIcons}
      <SidebarIcons
        repository={name}
        onfull={() => {
          showSidebar(false);
        }}
        {onsettings}
        count={workbench.count}
        reviewRequests={workbench.reviewRequests}
        staleReviews={workbench.staleReviews}
        filters={workbench.filters}
        selected={workbench.selected}
        onselect={showViewInFull}
      />
    {:else}
      <Sidebar
        repository={name}
        {onpick}
        {onsettings}
        onicons={() => {
          showSidebar(true);
        }}
        count={workbench.count}
        reviewRequests={workbench.reviewRequests}
        staleReviews={workbench.staleReviews}
        filters={workbench.filters}
        selected={workbench.selected}
        onselect={showView}
      />
    {/if}
    <!-- One edge for both shapes, so a drag that folds the Sidebar to icons can go on to unfold it. -->
    <PaneEdge
      label="Sidebar width"
      sizes={{
        kind: "pane",
        value: panes.sidebar ?? SIDEBAR_WIDTH,
        nextLeast: panes.hidden("list") ? DETAIL_LEAST : LIST_LEAST + (detail ? DETAIL_LEAST : 0),
      }}
      least={SIDEBAR_LEAST}
      most={SIDEBAR_MOST}
      fold={{
        folded: panes.sidebarIcons,
        below: SIDEBAR_FOLD,
        strip: SIDEBAR_ICONS,
        onfold: (icons) => {
          panes.foldSidebar(icons);
        },
      }}
      onresize={(width) => (panes.sidebar = width)}
      ondone={() => void panes.save()}
      onreset={() => void panes.reset("sidebar")}
    />
  {/if}
  {#if !panes.hidden("list")}
    <!-- The list keeps the width the user gave it beside the Detail pane, and fills the window without
         one. -->
    <section
      class={["list", { sized: panes.list !== null && detail !== undefined }]}
      data-pane="list"
      bind:this={list}
    >
      <header class="view">
        <h2>{workbench.title}</h2>
        <button
          class="link order"
          title="Change the order"
          onclick={() => void sortBy(panes.order === "attention" ? "last_update" : "attention")}
          >{ORDERS[panes.order]}</button
        >
      </header>
      {@render problems()}
      {#if workbench.items?.length === 0}
        <p>{EMPTY[workbench.selected.kind]}</p>
      {:else if workbench.items}
        <ul>
          {#each workbench.items as item (item.number)}
            {@render listed(item)}
          {/each}
        </ul>
      {/if}
    </section>
    {#if detail}
      <PaneEdge
        label="Workbench width"
        sizes={{ kind: "pane", value: panes.list ?? undefined, nextLeast: DETAIL_LEAST }}
        least={LIST_LEAST}
        onresize={(width) => (panes.list = width)}
        ondone={() => void panes.save()}
        onreset={() => void panes.reset("list")}
      />
    {/if}
  {/if}
  {#if detail instanceof IssueViewState}
    {@const issue = detail}
    {#key issue}
      <IssueDetail
        repository={name}
        {issue}
        {now}
        focusable={panes.hidden("list")}
        onmark={markByButton}
        onopenpr={(number) => {
          open({ number, kind: "pull_request", issue: issue.number });
        }}
      >
        {#snippet header()}{@render history(issue.number)}{/snippet}
      </IssueDetail>
    {/key}
  {:else if detail}
    {@const pr = detail}
    {#key pr}
      <PrDetail
        repository={name}
        {pr}
        {panes}
        {now}
        onback={back}
        onmark={markByButton}
        focusable={panes.hidden("list")}
      >
        {#snippet header()}{@render history(pr.number)}{/snippet}
      </PrDetail>
    {/key}
  {:else if panes.hidden("list")}
    {@const key = actionLabel("toggle_issue_list", currentOs())}
    <section class="empty" aria-label="Detail pane" tabindex="-1" data-pane="detail">
      <p>
        Nothing open. <button class="link" onclick={() => void toggleListByKey()}
          >{#if key === null}Show the Issue list{:else}<kbd>{key}</kbd>{/if}</button
        >{key === null ? "" : " shows the Issue list."}
      </p>
    </section>
  {/if}
</div>

<style>
  /* The panes take the window's height, and each scrolls by itself. The wheel stops at a pane's end, even
     when a short window lets the screen around the panes scroll too. */
  .home {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  /* The Detail pane grows from nothing, so the list keeps its width and is the first to shrink. A change that
     moves an item scrolls the list only to bring back the Detail pane's row (#796): the browser's scroll
     anchoring would scroll it whenever the rows above the first one on the screen shift. */
  .list {
    flex: 1;
    min-width: 280px;
    overflow-y: auto;
    overscroll-behavior: contain;
    overflow-anchor: none;
  }

  .list.sized {
    flex: 0 1 var(--list-width);
  }

  /* The Detail pane with nothing open, which shows only while the list is hidden. */
  .empty {
    flex: 1;
    min-width: 320px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* Out of sight and out of the layout, but still in the accessibility tree, as `display: none` isn't. */
  .hidden-name {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* The view's name, with the list's order at the far end, as the prototype has it. */
  .view {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
  }

  .view h2 {
    flex: 1;
    margin: 4px 0;
    font-size: 13px;
  }

  .order {
    font-size: 12px;
    color: var(--fg-muted);
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* An item reads like a message in a mail client: a line under each, and a bar on the left of one that
     Needs attention, as unread mail has. */
  .item {
    border-bottom: 1px solid var(--border);
    border-left: 3px solid transparent;
  }

  .item.attention {
    border-left-color: var(--accent);
  }

  .row {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 7px 10px 7px 12px;
    font-size: 13px;
  }

  .row:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  /* The row under the cursor, on the inactive selection until the list holds the focus. */
  .row:has(.title[aria-current]) {
    background: var(--selection-inactive);
  }

  .people,
  .facts {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .attention > .row .people {
    font-weight: 600;
  }

  .headline {
    display: flex;
    align-items: baseline;
    gap: 6px;
  }

  /* The title takes what room the line has left, and an ellipsis cuts it. */
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title::after {
    content: "";
    position: absolute;
    inset: 0;
  }

  /* The state icon sits above the title's cover, so its tooltip names the state, and a Stale review does too,
     so its tooltip lists the files. */
  .headline > :global(.octicon),
  .stale {
    position: relative;
    z-index: 1;
  }

  .stale {
    color: var(--danger);
  }

  /* The row's tint shows the pointer, and an underline across every title it passes would be noise. */
  .title:hover:not(:focus-visible) {
    text-decoration: none;
  }

  .title code {
    font-size: 12px;
  }

  .attention > .row .title {
    color: var(--accent);
  }

  .count,
  .age,
  .facts {
    font-size: 12px;
    color: var(--fg-muted);
  }

  .attention > .row .age {
    color: var(--accent);
  }

  /* › and ⌄ before an Issue with PRs, which fold and unfold its tree as ← and → do. It sits above the
     title's cover, so a click on it folds rather than opens. */
  .fold {
    all: unset;
    position: relative;
    z-index: 1;
    flex: none;
    width: 1em;
    text-align: center;
    cursor: pointer;
  }

  button.fold:hover,
  button.fold:focus-visible {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  /* A tree's rows sit under the item's headline, past its fold. */
  .pull-requests .row {
    padding-left: calc(12px + 1em + 6px);
  }

  .pull-requests .pull-requests .row {
    padding-left: calc(12px + 2 * (1em + 6px));
  }

  /* The focus shows as VS Code shows it (#602): a line along the top of the pane that holds it, and the
     row under the cursor in the active selection colour while that pane is the list. The line sticks to
     the top of the pane as it scrolls. Each pane keeps the line's room, so the line comes and goes without
     moving anything. */
  .home :global([data-pane])::before {
    content: "";
    position: sticky;
    top: 0;
    z-index: 2;
    display: block;
    flex: none;
    height: 2px;
  }

  /* With the focus on no element, as at launch, the list takes the keys, so it shows the line too. */
  .home.window-focused :global([data-pane]:focus-within)::before,
  :global(html:not(:has(:focus))) .home.window-focused .list::before {
    background: var(--focus-border);
  }

  .window-focused .list:focus-within .row:has(.title[aria-current]),
  :global(html:not(:has(:focus))) .window-focused .row:has(.title[aria-current]) {
    background: var(--selection-active);
  }

  /* The empty view's words and the problems line up with the header. */
  .list > p {
    margin: 8px 10px;
  }

  /* Windows' contrast themes draw every border in their own colour, the clear ones too, and drop the
     tints. Only an item that Needs attention keeps its bar, and the cursor's row gets an outline. */
  @media (forced-colors: active) {
    .item {
      border-left-color: Canvas;
    }

    .item.attention {
      border-left-color: Highlight;
    }

    .row:has(.title[aria-current]) {
      outline: 2px solid Highlight;
      outline-offset: -2px;
    }
  }
</style>
