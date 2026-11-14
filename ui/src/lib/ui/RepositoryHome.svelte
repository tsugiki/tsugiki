<script lang="ts">
  import { tick } from "svelte";
  import { openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import { hud } from "$lib/state/hud.svelte";
  import { IssueViewState } from "$lib/state/issue-view.svelte";
  import {
    DETAIL_LEAST,
    LIST_LEAST,
    type ListOrder,
    PanesState,
    SIDEBAR_LEAST,
    SIDEBAR_MOST,
    SIDEBAR_WIDTH,
  } from "$lib/state/panes.svelte";
  import { DiffViewChoice, type FilesMode, type PrFilesState } from "$lib/state/pr-files.svelte";
  import { PrViewState } from "$lib/state/pr-view.svelte";
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
  import HistoryBar from "./HistoryBar.svelte";
  import IssueDetail from "./IssueDetail.svelte";
  import { type ChordBindings, chordLabel, currentOs, isTextBox, keyLabel, keyName } from "./keys";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import type { PaletteAction } from "./Palette.svelte";
  import PaneEdge from "./PaneEdge.svelte";
  import PrDetail from "./PrDetail.svelte";
  import Sidebar from "./Sidebar.svelte";
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
    onadd,
    onpick,
    keysActive = true,
    diffFiles = "all",
  }: {
    name: string;
    onadd: () => void;
    /** Opens the Repository picker, from the Repository's name atop the Sidebar. */
    onpick: () => void;
    keysActive?: boolean;
    /** Whether each PR's Files tab opens on all its files or one at a time, as `[diff] files` says. */
    diffFiles?: FilesMode;
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
  const workbench = $derived(new WorkbenchState(name));
  /** The Issues and PRs the Detail pane has shown, which Alt+← and Alt+→ go back and forward through. Each
   * Repository has its own. */
  const visits = $derived.by(() => {
    void name;
    return new Visits<Stop>(sameStop);
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
  let quietLineButton = $state<HTMLButtonElement>();
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
  /** How many items back the history's lists go, newest first. */
  const RECENT = 12;

  /** Goes `steps` items through the history, back for a negative number, as Alt+← and Alt+→ do. */
  function goThrough(steps: number) {
    const place = visits.go(steps);
    if (place) open(place, "through");
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
   */
  function open(stop: Stop, how: "open" | "back" | "through" = "open") {
    if (how !== "through") visits.visit(stop);
    workbench.moveTo(stop);
    if (how !== "open") focusCursor();
    if (stop.kind === "pull_request" && detail instanceof PrViewState && sameStop(detail.line, stop)) {
      if (how !== "through") detail.switchTab();
      return;
    }
    if (stop.kind === "issue" && kept?.number === stop.number) {
      const resumed = kept;
      swap(resumed);
      if (how !== "open") {
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
        : new PrViewState(name, stop.number, stop.issue, panes, diffView, diffFiles);
    swap(next);
    void next.load();
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

  /** Ctrl+B: hides the Sidebar, or brings it back at its width. Focus in the Sidebar moves to the cursor's
   * row, so it doesn't fall out of the window with the Sidebar. */
  async function toggleSidebarByKey() {
    const focused = document.activeElement;
    void panes.toggle("sidebar");
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

  /** Whether a key pressed on `target` goes to the Detail pane: the pane holds the focus, or the list is
   * hidden. ↑ ↓ then scroll it, and `Q` marks its item. */
  const forDetail = (target: EventTarget | null) =>
    panes.hidden("list") || (target instanceof Node && detailPane()?.contains(target) === true);

  /** The item `Q` and `Shift+Q` act on: the Detail pane's item while it holds the focus or the list is
   * hidden, and otherwise the line under the cursor. */
  function keyMarked(target: EventTarget | null): Marked | undefined {
    if (forDetail(target)) return detailMarked();
    const stop = workbench.underCursor;
    const seen = stop && workbench.seenOf(stop);
    return stop && seen ? { stop, number: stop.number, seen } : undefined;
  }

  /** Mark Seen on `marked`, as the window drew it. A cursor on its row lands on the row that takes its place
   * once it moves, and takes the focus there when `focus` says so. */
  async function markSeen(marked: Marked | undefined, focus = false) {
    if (!marked) return;
    markProblem = await workbench.markSeen(marked.seen.mark, marked.stop, () => {
      if (focus) focusCursor();
    });
  }

  async function markUnseen(marked: Marked | undefined) {
    if (marked) markProblem = await workbench.markUnseen(marked.number);
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

  /** The chords this screen binds, each with the action the page runs once the chord's wait ends. */
  export function chordBindings(): ChordBindings {
    return {
      "ctrl+k j": () => {
        shownFiles()?.unfoldUnchanged();
      },
      "ctrl+k o": () => {
        shownFiles()?.toggleMode();
      },
      "ctrl+k z": () => void toggleDetailOnly(),
    };
  }

  /** The Files tab of the PR on show, while it shows its diff. The file view covers the diff, so the diff's
   * keys and commands wait until Back (#453). */
  function shownFiles(): PrFilesState | undefined {
    const files = detail instanceof PrViewState && detail.tab === "files" ? detail.files : undefined;
    return files?.fileView === undefined ? files : undefined;
  }

  /** The palette's commands for the Files tab on show: the one that switches it to one file at a time, or
   * back, and while files unchanged since the user's Stale review are folded, the one that opens them
   * (#387). */
  function filesCommands(): PaletteAction[] {
    const files = shownFiles();
    if (files === undefined) return [];
    const unfold = {
      title: "View: Unfold Unchanged Files",
      note: chordLabel("ctrl+k j", currentOs()),
      run: () => {
        files.unfoldUnchanged();
      },
    };
    return [
      {
        title: files.mode === "one" ? "View: All Files" : "View: One File at a Time",
        note: chordLabel("ctrl+k o", currentOs()),
        run: () => {
          files.toggleMode();
        },
      },
      ...(files.foldedUnchanged > 0 ? [unfold] : []),
    ];
  }

  /** What this screen adds to the command palette while it shows. Each command runs what its key runs. The
   * palette runs it once it has handed the focus back, so the focus goes where the key would put it. */
  export function paletteCommands(): PaletteAction[] {
    return [
      {
        title: panes.hidden("sidebar") ? "Show Sidebar" : "Hide Sidebar",
        run: () => void toggleSidebarByKey(),
      },
      {
        title: panes.hidden("list") ? "Show Issue List" : "Hide Issue List",
        run: () => void toggleListByKey(),
      },
      {
        title: panes.detailOnly ? "Show the Panes" : "Show Only the Detail Pane",
        run: () => void toggleDetailOnly(),
      },
      { title: "Reset Layout", run: () => void resetLayout() },
      {
        title: "Go Back",
        note: keyLabel("alt+arrowleft", currentOs()),
        run: () => {
          goThrough(-1);
        },
      },
      {
        title: "Go Forward",
        note: keyLabel("alt+arrowright", currentOs()),
        run: () => {
          goThrough(1);
        },
      },
      ...(detail?.view
        ? [
            {
              title: "Mark Seen",
              note: keyLabel("q", currentOs()),
              run: () => void markSeen(detailMarked()),
            },
            {
              title: "Mark Unseen",
              note: keyLabel("shift+q", currentOs()),
              run: () => void markUnseen(detailMarked()),
            },
          ]
        : []),
      panes.order === "attention"
        ? { title: "Sort by Last Update", run: () => void sortBy("last_update") }
        : { title: "Sort by Attention", run: () => void sortBy("attention") },
      ...(workbench.quiet.length > 0
        ? [{ title: workbench.quietShown ? "Hide Quiet Items" : "Show Quiet Items", run: toggleQuiet }]
        : []),
      { title: "View: Stale Reviews", run: () => showView({ kind: "stale_reviews" }) },
      ...filesCommands(),
    ];
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
    await workbench.refresh();
  }

  /** Unfolds the quiet section, or folds it. A cursor in it moves to the last line above it, and takes the
   * focus with it if the focus was on a row that went away. With no line above, the focus goes to the
   * line that unfolds the section again. */
  function toggleQuiet() {
    if (!workbench.quietShown) {
      workbench.showQuiet();
      return;
    }
    const focused = document.activeElement;
    workbench.hideQuiet();
    void tick().then(() => {
      if (focused?.isConnected !== false) return;
      if (workbench.cursor) focusCursor();
      else quietLineButton?.focus();
    });
  }

  /** The line that folds the quiet section: what a click on it does, and to how many items. */
  const quietLine = (shown: boolean, count: number) =>
    `${shown ? "Hide" : "Show"} ${count === 1 ? "1 quiet item" : `${String(count)} quiet items`}`;

  /** Names a line for the page, which can list a PR under two Issues. */
  const lineName = (stop: Stop | undefined) => `${String(stop?.issue ?? "")}#${String(stop?.number)}`;

  function moveCursor(step: 1 | -1) {
    workbench.moveCursor(step);
    focusCursor();
  }

  /** Whether a key went to the list, or to no control at all, while the list shows. */
  const inList = (target: EventTarget | null) =>
    !panes.hidden("list") &&
    (target === document.body || (target instanceof HTMLElement && target.closest(".list") !== null));

  /** → unfolds the Issue under the cursor, and ← folds it. ← on a PR in an Issue's tree moves the cursor to
   * the Issue, as in VS Code's trees (#593). A PR outside any Issue has nothing to fold. */
  function foldByKey(key: "arrowright" | "arrowleft") {
    const line = workbench.underCursor;
    if (line?.kind === "issue") {
      if (key === "arrowright") workbench.unfold(line.number);
      else workbench.fold(line.number);
    } else if (line?.issue !== undefined && key === "arrowleft") {
      workbench.moveTo({ number: line.issue, kind: "issue", issue: undefined });
      focusCursor();
    }
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

  /** Scrolls the Detail pane by a line, or the diff on a PR's Files tab, or the file view over it, each of
   * which scrolls by itself. */
  function scrollDetail(step: 1 | -1) {
    const pane = detailPane();
    // A region something covers, such as the diff under the file view, is inert and keeps still.
    const scroller = pane?.querySelector<HTMLElement>("[data-scrolls]:not([inert] *)") ?? pane;
    scroller?.scrollBy({ top: step * LINE });
  }

  /**
   * The list's keys and the PR view's. A dialog on top keeps them, and a text box or a focused control
   * keeps its own. The window's guard has already stopped the browser for Ctrl+B, Ctrl+Shift+G, Ctrl+Alt+U
   * and Ctrl+F, which work in a text box too, and for Ctrl+Alt+B outside one, as AltGr sends it to type.
   * ↑ and ↓ go where the focus line shows (#602): they scroll the Detail pane while it holds the focus or
   * the list is hidden, and move the list's cursor otherwise. With the list hidden, Enter has no row to open.
   */
  function onkeydown(e: KeyboardEvent) {
    if (!keysActive) return;
    const key = keyName(e, currentOs());
    const files = shownFiles();
    if (key === "ctrl+b") {
      // A held key would flicker the Sidebar and save the file each time.
      if (!e.repeat) void toggleSidebarByKey();
    } else if (key === "ctrl+shift+g") {
      const current = currentOnGitHub();
      if (current) void onGitHub(current);
    } else if (key === "ctrl+alt+u" && files) {
      e.preventDefault();
      diffView.toggle();
    } else if (key === "ctrl+f" && files) {
      files.openSearch();
    } else if (isTextBox(e.target)) {
      return;
    } else if (key === "q" || key === "shift+q") {
      // A held key would ask the core again and again.
      if (e.repeat) return;
      const marked = keyMarked(e.target);
      if (key === "q") void markSeen(marked, !forDetail(e.target));
      else void markUnseen(marked);
    } else if (key === "ctrl+alt+b") {
      // A held key would flicker the list and save the file each time.
      if (!e.repeat) void toggleListByKey();
    } else if (key === "enter" && e.repeat) {
      // Each opening asks GitHub, and a held key isn't more than one.
      e.preventDefault();
    } else if ((key === "arrowdown" || key === "arrowup") && forDetail(e.target)) {
      e.preventDefault();
      scrollDetail(key === "arrowdown" ? 1 : -1);
    } else if (key === "arrowdown" || key === "arrowup") {
      e.preventDefault();
      moveCursor(key === "arrowdown" ? 1 : -1);
    } else if ((key === "arrowright" || key === "arrowleft") && inList(e.target)) {
      // Elsewhere the bare arrows are a pane edge's, or scroll a diff or a code block sideways.
      e.preventDefault();
      foldByKey(key);
    } else if (
      key === "enter" &&
      e.target === document.body &&
      workbench.underCursor &&
      !panes.hidden("list")
    ) {
      e.preventDefault();
      open(workbench.underCursor);
    } else if (key === "escape" && (files?.peek || files?.going)) {
      // Escape closes the Definition peek first, or stops Go to definition, then the search, as in VS Code.
      // The search box keeps its own Escape, which closes the search.
      e.preventDefault();
      files.closePeek();
    } else if (key === "escape" && files?.searching) {
      e.preventDefault();
      files.closeSearch();
    } else if (key === "escape" && detail instanceof PrViewState && detail.issue !== undefined) {
      // A PR opened from a row of its own has nothing under it, so it stays on show.
      e.preventDefault();
      back(detail.issue);
    } else if ((key === "ctrl+pagedown" || key === "ctrl+pageup") && detail instanceof PrViewState) {
      e.preventDefault();
      detail.switchTab();
    } else if ((key === "alt+pagedown" || key === "alt+pageup") && files) {
      e.preventDefault();
      files.nextFile(key === "alt+pagedown" ? 1 : -1);
    } else if ((key === "f8" || key === "shift+f8") && files) {
      e.preventDefault();
      files.nextThread(key === "f8" ? 1 : -1);
    } else if ((key === "f7" || key === "shift+f7") && files) {
      e.preventDefault();
      files.nextChange(key === "f7" ? 1 : -1);
    } else if (key === "alt+f12" && files) {
      // A held key would ask ty again and again.
      e.preventDefault();
      if (!e.repeat) void files.peekDefinition();
    } else if (key === "f12" && files) {
      e.preventDefault();
      if (!e.repeat) void files.goToDefinition();
    } else if (key === "alt+arrowleft" || key === "alt+arrowright") {
      e.preventDefault();
      goThrough(key === "alt+arrowleft" ? -1 : 1);
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
    back={visits.behind(RECENT).map((stop) => headlineOf(stop.number))}
    forward={visits.ahead(RECENT).map((stop) => headlineOf(stop.number))}
    current={headlineOf(shown)}
    ongo={goThrough}
  />
{/snippet}

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
    <Sidebar
      repository={name}
      {onpick}
      count={workbench.count}
      reviewRequests={workbench.reviewRequests}
      staleReviews={workbench.staleReviews}
      filters={workbench.filters}
      selected={workbench.selected}
      onselect={showView}
    />
    <PaneEdge
      label="Sidebar width"
      sizes={{
        kind: "pane",
        value: panes.sidebar ?? SIDEBAR_WIDTH,
        nextLeast: panes.hidden("list") ? DETAIL_LEAST : LIST_LEAST + (detail ? DETAIL_LEAST : 0),
      }}
      least={SIDEBAR_LEAST}
      most={SIDEBAR_MOST}
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
      <h1>{name}</h1>
      <button onclick={onadd}>Add repository</button>
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
      {#if workbench.items?.length === 0 && workbench.quiet.length === 0}
        <p>{EMPTY[workbench.selected.kind]}</p>
      {:else if workbench.items}
        <ul>
          {#each workbench.items as item (item.number)}
            {@render listed(item)}
          {/each}
        </ul>
        {#if workbench.quiet.length > 0}
          <button
            class="link quiet small-heading"
            aria-expanded={workbench.quietShown}
            onclick={toggleQuiet}
            bind:this={quietLineButton}
          >
            {quietLine(workbench.quietShown, workbench.quiet.length)}
          </button>
          {#if workbench.quietShown}
            <ul aria-label="Quiet items">
              {#each workbench.quiet as item (item.number)}
                {@render listed(item)}
              {/each}
            </ul>
          {/if}
        {/if}
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
        {now}
        onback={back}
        onmark={markByButton}
        focusable={panes.hidden("list")}
      >
        {#snippet header()}{@render history(pr.number)}{/snippet}
      </PrDetail>
    {/key}
  {:else if panes.hidden("list")}
    <section class="empty" aria-label="Detail pane" tabindex="-1" data-pane="detail">
      <p>
        Nothing open. <button class="link" onclick={() => void toggleListByKey()}
          ><kbd>{keyLabel("ctrl+alt+b", currentOs())}</kbd></button
        > shows the Issue list.
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
  /* The Detail pane grows from nothing, so the list keeps its width and is the first to shrink. */
  .list {
    flex: 1;
    min-width: 280px;
    overflow-y: auto;
    overscroll-behavior: contain;
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

  h1 {
    margin: 8px 10px;
    font-size: 16px;
  }

  h1 + button {
    margin: 0 10px 8px;
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

  .quiet {
    display: block;
    padding: 12px 10px 4px;
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
