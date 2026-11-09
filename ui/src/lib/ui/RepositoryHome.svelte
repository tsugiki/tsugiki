<script lang="ts">
  import { tick } from "svelte";
  import { openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import { hud } from "$lib/state/hud.svelte";
  import { IssueViewState } from "$lib/state/issue-view.svelte";
  import {
    DETAIL_LEAST,
    LIST_LEAST,
    PanesState,
    SIDEBAR_LEAST,
    SIDEBAR_MOST,
    SIDEBAR_WIDTH,
  } from "$lib/state/panes.svelte";
  import { DiffViewChoice } from "$lib/state/pr-files.svelte";
  import { PrViewState } from "$lib/state/pr-view.svelte";
  import { Visits } from "$lib/state/visits";
  import {
    sameStop,
    WorkbenchState,
    type BackportRow,
    type ListedItem,
    type Stop,
    type WorkbenchRow,
  } from "$lib/state/workbench.svelte";
  import IssueDetail from "./IssueDetail.svelte";
  import { currentOs, isTextBox, keyName } from "./keys";
  import { issueIcon, prIcon, type Octicon } from "./octicons";
  import type { PaletteCommand } from "./Palette.svelte";
  import PaneEdge from "./PaneEdge.svelte";
  import PrDetail from "./PrDetail.svelte";
  import Sidebar from "./Sidebar.svelte";
  import StateIcon from "./StateIcon.svelte";
  import { details, headline, issueDetails, preview, standaloneDetails } from "./workbench-row";

  const MINUTE = 60_000;
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
  };

  let {
    name,
    onadd,
    keysActive = true,
  }: { name: string; onadd: () => void; keysActive?: boolean } = $props();

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
  let list = $state<HTMLElement>();

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(name, item);
  };

  /**
   * Shows the Issue or PR on line `stop` in the Detail pane. The PR already on show switches tabs instead,
   * and the Issue already on show reads GitHub again. Each counts as a visit, unless Alt+← or Alt+→ went
   * `"through"` the visits to it. Going `"back"` to the Issue the PR on show came from, or through the
   * visits to it, shows its view as it was and counts as no new opening. Both move the focus with the
   * cursor, so the row the user left doesn't look selected too, and Enter doesn't open it again.
   */
  function open(stop: Stop, how: "open" | "back" | "through" = "open") {
    if (how !== "through") visits.visit(stop);
    workbench.cursor = stop;
    if (how !== "open") focusCursor();
    if (stop.kind === "pull_request" && detail instanceof PrViewState && sameStop(detail.line, stop)) {
      detail.switchTab();
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
    hud.issueOpened(stop.number);
    if (stop.kind === "issue" && detail instanceof IssueViewState && detail.number === stop.number) {
      // Opening leaves a view that was hidden nothing to catch up on when it shows.
      void detail.open();
      detail.show();
      return;
    }
    const next =
      stop.kind === "issue"
        ? new IssueViewState(name, stop.number)
        : new PrViewState(name, stop.number, stop.issue, diffView);
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

  /** The Issue or PR in the Detail pane, or else the line under the cursor. */
  function currentOnGitHub(): GitHubItem | undefined {
    if (detail instanceof IssueViewState) return { kind: "issue", number: detail.number };
    if (detail) return { kind: "pull_request", number: detail.number };
    const stop = workbench.underCursor;
    return stop && { kind: stop.kind, number: stop.number };
  }

  /** Ctrl+B: hides the Sidebar, or brings it back at its width. Focus in the Sidebar moves to the cursor's
   * row, so it doesn't fall out of the window with the Sidebar. */
  async function toggleSidebarByKey() {
    const focused = document.activeElement;
    void panes.toggleSidebar();
    await tick();
    if (focused?.isConnected === false) focusCursor();
  }

  /** What this screen adds to the command palette while it shows. Where the focus goes as the palette
   * closes is the palette's to say. */
  export function paletteCommands(): PaletteCommand[] {
    return [
      {
        title: panes.sidebarHidden ? "Show Sidebar" : "Hide Sidebar",
        run: () => void panes.toggleSidebar(),
      },
    ];
  }

  /** Names a line for the page, which can list a PR under two Issues. */
  const lineName = (stop: Stop | undefined) => `${String(stop?.issue ?? "")}#${String(stop?.number)}`;

  function moveCursor(step: 1 | -1) {
    workbench.moveCursor(step);
    focusCursor();
  }

  /** Focuses the row under the cursor, so it scrolls into view, and Enter presses its button. */
  function focusCursor() {
    list?.querySelector<HTMLElement>(`[data-item="${lineName(workbench.cursor)}"]`)?.focus();
  }

  /**
   * The list's keys and the PR view's. A dialog on top keeps them, and a text box or a focused control
   * keeps its own. The window's guard has already stopped the browser for Ctrl+B, Ctrl+Shift+G, Ctrl+Alt+U
   * and Ctrl+F, which work in a text box too.
   */
  function onkeydown(e: KeyboardEvent) {
    if (!keysActive) return;
    const key = keyName(e, currentOs());
    const files = detail instanceof PrViewState && detail.tab === "files" ? detail.files : undefined;
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
    } else if (key === "enter" && e.repeat) {
      // Each opening asks GitHub, and a held key isn't more than one.
      e.preventDefault();
    } else if (key === "arrowdown" || key === "arrowup") {
      e.preventDefault();
      moveCursor(key === "arrowdown" ? 1 : -1);
    } else if (key === "enter" && e.target === document.body && workbench.underCursor) {
      e.preventDefault();
      open(workbench.underCursor);
    } else if (key === "escape" && files?.searching) {
      // Escape closes the search first, wherever the focus is, as in VS Code.
      e.preventDefault();
      files.closeSearch();
    } else if (key === "escape" && detail instanceof PrViewState && detail.issue !== undefined) {
      // A PR opened from a row of its own has nothing under it, so it stays on show.
      e.preventDefault();
      back(detail.issue);
    } else if ((key === "ctrl+pagedown" || key === "ctrl+pageup") && detail instanceof PrViewState) {
      e.preventDefault();
      detail.switchTab();
    } else if ((key === "f8" || key === "shift+f8") && files) {
      e.preventDefault();
      files.nextThread(key === "f8" ? 1 : -1);
    } else if ((key === "f7" || key === "shift+f7") && files) {
      e.preventDefault();
      files.nextChange(key === "f7" ? 1 : -1);
    } else if (key === "alt+arrowleft" || key === "alt+arrowright") {
      e.preventDefault();
      const place = key === "alt+arrowleft" ? visits.back() : visits.forward();
      if (place) open(place, "through");
    }
  }
</script>

{#snippet line(stop: Stop, text: string, strong: boolean, icon: Octicon | undefined)}
  <!-- The icon sits beside the button, so the button's name stays the item's headline. -->
  <span>
    {#if icon}
      <StateIcon {icon} />
    {/if}
    <button
      class="open"
      data-item={lineName(stop)}
      aria-current={sameStop(workbench.cursor, stop) ? "true" : undefined}
      onclick={() => {
        open(stop);
      }}
    >
      {#if strong}
        <strong>{text}</strong>
      {:else}
        <span>{text}</span>
      {/if}
    </button>
  </span>
{/snippet}

{#snippet pullRequest(row: WorkbenchRow, issue: number | undefined)}
  {@render line({ number: row.number, kind: "pull_request", issue }, headline(row), false, prIcon(row))}
{/snippet}

{#snippet backports(rows: BackportRow[], issue: number | undefined)}
  {#if rows.length > 0}
    <ul class="pull-requests">
      {#each rows as row (row.number)}
        <li>
          {@render pullRequest(row, issue)}
          <small>{details(row)}</small>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet listed(item: ListedItem)}
  <li>
    {@render line(
      { number: item.number, kind: item.kind === "issue" ? "issue" : "pull_request", issue: undefined },
      headline(item),
      item.attention !== null,
      item.kind === "issue" ? issueIcon(item.state) : prIcon(item),
    )}
    {#if item.attention}
      <small class="preview">{preview(item.attention)}</small>
    {/if}
    {#if item.kind === "issue"}
      <small>{issueDetails(item, now)}</small>
      <ul class="pull-requests">
        {#each item.pull_requests as entry (entry.number)}
          <li>
            {@render pullRequest(entry, item.number)}
            <small>{details(entry)}</small>
            {#if entry.kind === "pr"}
              {@render backports(entry.backports, item.number)}
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <small>{standaloneDetails(item, now)}</small>
      {#if item.kind === "standalone_pr"}
        {@render backports(item.backports, undefined)}
      {/if}
    {/if}
  </li>
{/snippet}

<svelte:window {onkeydown} />

<div
  class="home"
  style:--sidebar-width={`${String(panes.sidebar ?? SIDEBAR_WIDTH)}px`}
  style:--list-width={panes.list === null ? undefined : `${String(panes.list)}px`}
>
  {#if !panes.sidebarHidden}
    <Sidebar
      count={workbench.count}
      reviewRequests={workbench.reviewRequests}
      filters={workbench.filters}
      selected={workbench.selected}
      onselect={(view) => void workbench.select(view)}
    />
    <PaneEdge
      label="Sidebar width"
      value={panes.sidebar ?? SIDEBAR_WIDTH}
      least={SIDEBAR_LEAST}
      most={SIDEBAR_MOST}
      nextLeast={LIST_LEAST + (detail ? DETAIL_LEAST : 0)}
      onresize={(width) => (panes.sidebar = width)}
      ondone={() => void panes.save()}
      onreset={() => void panes.reset("sidebar")}
    />
  {/if}
  <!-- The list keeps the width the user gave it beside the Detail pane, and fills the window without one. -->
  <section class={["list", { sized: panes.list !== null && detail !== undefined }]} bind:this={list}>
    <h1>{name}</h1>
    <button onclick={onadd}>Add repository</button>
    {#if workbench.error}
      <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
      <p role="alert">{`Tsugiki can't read this Workbench: ${workbench.error}`}</p>
    {/if}
    {#if browserProblem}
      <p role="alert">{browserProblem}</p>
    {/if}
    {#if panes.problem}
      <p role="alert">{panes.problem}</p>
    {/if}
    {#if workbench.items?.length === 0 && workbench.quiet.length === 0}
      <p>{EMPTY[workbench.selected.kind]}</p>
    {:else if workbench.items}
      <ul>
        {#each workbench.items as item (item.number)}
          {@render listed(item)}
        {/each}
      </ul>
      {#if workbench.quiet.length > 0}
        <h2>Quiet</h2>
        <ul>
          {#each workbench.quiet as item (item.number)}
            {@render listed(item)}
          {/each}
        </ul>
      {/if}
    {/if}
  </section>
  {#if detail}
    <PaneEdge
      label="Workbench width"
      value={panes.list ?? undefined}
      least={LIST_LEAST}
      nextLeast={DETAIL_LEAST}
      onresize={(width) => (panes.list = width)}
      ondone={() => void panes.save()}
      onreset={() => void panes.reset("list")}
    />
  {/if}
  {#if detail instanceof IssueViewState}
    {@const issue = detail}
    {#key issue}
      <IssueDetail
        repository={name}
        {issue}
        {now}
        onopenpr={(number) => {
          open({ number, kind: "pull_request", issue: issue.number });
        }}
      />
    {/key}
  {:else if detail}
    {@const pr = detail}
    {#key pr}
      <PrDetail repository={name} {pr} {now} onback={back} />
    {/key}
  {/if}
</div>

<style>
  .home {
    display: flex;
  }
  /* The Detail pane grows from nothing, so the list keeps its width and is the first to shrink. */
  .list {
    flex: 1;
    min-width: 280px;
  }

  .list.sized {
    flex: 0 1 var(--list-width);
  }

  h1 {
    font-size: 16px;
  }

  .open {
    all: unset;
    cursor: pointer;
  }

  .open:hover,
  .open:focus-visible,
  .open[aria-current] {
    text-decoration: underline;
  }

  h2 {
    font-size: 13px;
    margin: 16px 0 0;
  }

  .preview {
    font-weight: 600;
  }

  ul {
    list-style: none;
    margin: 12px 0 0;
    padding: 0;
  }

  li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 0;
  }

  .pull-requests {
    margin-top: 2px;
    padding-left: 16px;
  }
</style>
