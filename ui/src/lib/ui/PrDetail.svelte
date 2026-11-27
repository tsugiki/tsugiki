<script lang="ts">
  import { type Snippet, untrack } from "svelte";
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import {
    BESIDE_RAIL_LEAST,
    type PanesState,
    RAIL_LEAST,
    RAIL_MOST,
    RAIL_WIDTH,
  } from "$lib/state/panes.svelte";
  import type { PrTab, PrView, PrViewState } from "$lib/state/pr-view.svelte";
  import { RAIL_TABS, type RailTab, railTabLabel, type RailTabId } from "$lib/state/rail.svelte";
  import type { RepositoryState } from "$lib/state/repository-state.svelte";
  import Conversation from "./Conversation.svelte";
  import FilesTab from "./FilesTab.svelte";
  import { focusBesideRail } from "./focus";
  import HideButtons from "./HideButtons.svelte";
  import { currentOs, withKey } from "./keys";
  import MarkButton from "./MarkButton.svelte";
  import { prIcon } from "./octicons";
  import PaneEdge from "./PaneEdge.svelte";
  import PersonLink from "./PersonLink.svelte";
  import Rail from "./Rail.svelte";
  import RailFiles from "./RailFiles.svelte";
  import RailSearch from "./RailSearch.svelte";
  import ReadOnlyNote from "./ReadOnlyNote.svelte";
  import StateIcon from "./StateIcon.svelte";
  import StateWords from "./StateWords.svelte";
  import { PR_UNREAD } from "./unread-words";
  import { ago, headline, staleReviewLine, updatedTip } from "./workbench-row";

  let {
    repository,
    pr,
    panes,
    now,
    onback,
    onmark,
    onarchive,
    onmute,
    onmuteauthor,
    onperson = () => {},
    marks = true,
    focusable = false,
    header,
    repositoryState,
  }: {
    repository: string;
    pr: PrViewState;
    /** The layout of the panes, which says whether the rail shows and how wide. */
    panes: PanesState;
    now: Date;
    onback: (issue: number) => void;
    /** Mark Seen or Mark Unseen, whichever the header's button offers. */
    onmark: () => void;
    /** Archive, or Unarchive on an Archived item, as the header's button offers for the PR on show
     * (#490). */
    onarchive: (view: PrView) => void;
    /** Mute, or Unmute on a Muted item, as the header's button offers for the PR on show (#492). */
    onmute: (view: PrView) => void;
    /** Mute author on the PR's author, or Unmute author on a Muted author, as the header's button offers
     * for the PR on show (#494). */
    onmuteauthor: (view: PrView) => void;
    /** Fills the Inbox's search box with the search for a clicked name (#472). */
    onperson?: (login: string) => void;
    /** Whether the header offers Mark Seen or Mark Unseen, which it doesn't on a PR of a Muted item
     * (#492). */
    marks?: boolean;
    /** Whether the pane takes the focus itself: while the Issue list is hidden, and while the list shows
     * after a click in its controls or a press of F6. Otherwise a click on its background drops the focus to
     * the page, which the list's keys take, so Enter still opens the row under the cursor. */
    focusable?: boolean;
    /** What sits at the top of the pane, such as the history's arrows (#586). */
    header?: Snippet;
    /** The Repository's state, which says atop the view why Tsugiki can't write there (#398). */
    repositoryState?: RepositoryState | undefined;
  } = $props();

  const TABS: { tab: PrTab; name: string }[] = [
    { tab: "conversation", name: "Conversation" },
    { tab: "files", name: "Files" },
  ];

  const links = new LinkOpener();
  // Svelte sets each to null as its element goes.
  let section = $state<HTMLElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  /** The rail's tabs, each with the pane it draws. Derived, so that a count a tab takes from the PR, as
   * #413's Review will, follows it. */
  const railTabs: Readonly<Record<RailTabId, RailTab>> = $derived({
    files: { ...RAIL_TABS.files, pane: filesPane },
    search: { ...RAIL_TABS.search, pane: searchPane },
  });
  /** Whether the rail shows: it opens beside the tabs, which come with the PR's view. */
  const railed = $derived(panes.railState.open && Boolean(pr.view));
  /** The Issue the crumb goes back to: the one the user opened the PR from, or else the first whose tree its
   * view names (#1077). */
  const backIssue = $derived(pr.returnIssue);
  /** Whether the Conversation scrolls by itself, as it does beside the rail. Without the rail the whole pane
   * scrolls it, under the header. */
  const conversationScrolls = $derived(railed && pr.tab === "conversation");

  /** The pixels of the pane above the tab, which scroll away with the Conversation while the pane scrolls
   * it. */
  const headerHeight = (pane: HTMLElement, tab: HTMLElement) =>
    tab.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;

  /** How far down the Conversation the user has read, from the scroller the page has now. */
  function conversationTop(): number {
    if (section === null || panel === null || pr.tab !== "conversation") return 0;
    if (panel.hasAttribute("data-scrolls")) return panel.scrollTop;
    return Math.max(0, section.scrollTop - headerHeight(section, panel));
  }

  /** Scrolls the Conversation `top` pixels down, in the scroller the page has now. Back at the top, the pane
   * shows its header again. */
  function showConversationAt(top: number) {
    if (section === null || panel === null || pr.tab !== "conversation") return;
    if (panel.hasAttribute("data-scrolls")) panel.scrollTop = top;
    else section.scrollTop = top > 0 ? top + headerHeight(section, panel) : 0;
  }

  // Opening or closing the rail, or moving to the other tab, changes what scrolls. Opening or closing it on
  // the Conversation moves the Conversation's scroller between the pane and the tab, which would start it at
  // the top, so each reads where the Conversation was before the page changes, and scrolls the new scroller
  // there after. And the Conversation beside the rail holds the focus as only a scroller does, so a focus on
  // it that the change leaves on a tab that no longer scrolls goes to what the rail sits beside, or the
  // button of the rail's tab: on the page, the arrows would move the Issue list's cursor.
  let readTo = 0;
  let panelFocused = false;
  $effect.pre(() => {
    void [railed, pr.tab];
    untrack(() => {
      readTo = conversationTop();
      panelFocused = panel !== null && document.activeElement === panel;
    });
  });
  $effect(() => {
    void [railed, pr.tab];
    untrack(() => {
      showConversationAt(readTo);
      if (panelFocused && !conversationScrolls) focusBesideRail(section, panes.railState.tab);
      handOnFocus();
    });
  });

  /** The pane takes the focus while the Issue list is hidden, but beside the rail the Conversation scrolls
   * and the pane doesn't, so the tab takes the pane's focus on, for Page Down and the like: as the pane takes
   * it, and as the rail opens while the pane holds it. */
  function handOnFocus() {
    if (document.activeElement === section && conversationScrolls) panel?.focus({ preventScroll: true });
  }
</script>

<!-- The rail's Files tab: the tree of the PR's changed files (#604). -->
{#snippet filesPane()}<RailFiles {pr} />{/snippet}

<!-- The rail's Search tab, over the PR's code (#686). -->
{#snippet searchPane()}<RailSearch {pr} />{/snippet}

<section
  bind:this={section}
  class="detail-pane"
  class:files={pr.tab === "files"}
  class:railed
  aria-label="PR view"
  tabindex={focusable ? -1 : undefined}
  data-pane="detail"
  onfocus={handOnFocus}
>
  {@render header?.()}
  <ReadOnlyNote {repositoryState} {repository} />
  {#if pr.error}
    <p role="alert">{`Tsugiki can't show this PR: ${pr.error}`}</p>
  {/if}
  {#if links.problem}
    <p role="alert">{links.problem}</p>
  {/if}
  <!-- The line over the title: the Issue the PR goes back to, where it goes and when it changed. -->
  {#if backIssue !== undefined || pr.view}
    <p class="crumb">
      {#if backIssue !== undefined}
        {@const issue = backIssue}
        <button
          class="link back"
          onclick={() => {
            onback(issue);
          }}>{`gh-${String(issue)}`}</button
        >
        <span aria-hidden="true">›</span>
      {/if}
      {#if pr.view}
        <span title={updatedTip(pr.view)}
          >{`into ${pr.view.target_branch} · updated ${ago(pr.view.updated_at, now)}`}</span
        >
      {/if}
    </p>
  {/if}
  {#if pr.view === null}
    <p>{`Tsugiki doesn't hold PR #${String(pr.number)}.`}</p>
  {:else if pr.view}
    {@const view = pr.view}
    {@const stale = staleReviewLine(view.your_review, view.shares_review_of)}
    <h2 class="title-line">
      <span class="state-icon"><StateIcon icon={prIcon(view)} /></span>
      <button
        class="link"
        onclick={() => void links.openOnGitHub(repository, { kind: "pull_request", number: view.number })}
      >
        {headline(view)}
      </button>
    </h2>
    <p class="byline">
      <b
        >{#if view.author}<PersonLink login={view.author} onopen={onperson} />{:else}deleted user{/if}</b
      >
      <StateWords row={view} />
    </p>
    <!-- A PR no Issue's tree holds offers the actions of the item it shows: its own, or the Standalone PR's
         over its Backport (#1077). A PR in an Issue's tree shows only Mark Seen, as its `gh-N` crumb opens
         the Issue view, where the Issue's buttons are. -->
    <MarkButton seen={view.seen} {onmark} {marks}>
      {#if view.issues.length === 0}
        <HideButtons {view} {onarchive} {onmute} {onmuteauthor} />
      {/if}
    </MarkButton>

    <!-- The tabs, and at the right a button for each of the rail's tabs, which opens the rail beside them on
         that tab, or closes it there. -->
    <div class="tab-row">
      <div class="tabs" role="tablist">
        {#each TABS as { tab, name } (tab)}
          <button
            role="tab"
            id={`pr-tab-${tab}`}
            aria-selected={pr.tab === tab}
            aria-controls="pr-tab-panel"
            onclick={() => {
              pr.tab = tab;
            }}
          >
            {name}
          </button>
        {/each}
      </div>
      {#each panes.railState.order as id (id)}
        {@const label = railTabLabel(railTabs[id])}
        <button
          type="button"
          class="rail-button"
          data-opens-rail={id}
          aria-label={`${railTabs[id].name} in the rail`}
          aria-pressed={panes.railState.open && panes.railState.tab === id}
          title={withKey(`${label} in the rail`, railTabs[id].action, currentOs())}
          onclick={() => {
            panes.railState.toggle(id);
          }}>{label}</button
        >
      {/each}
    </div>
    {#if stale !== undefined}
      <p class="stale">
        <span title={stale}>{stale}</span>
        <!-- A Bot backport's verdict read its Origin PR's commits, so its own files carry no marks. -->
        {#if view.your_review.kind === "stale_review" && view.shares_review_of === null}
          <button
            type="button"
            onclick={() => {
              pr.reviewChanges();
            }}>Review changes</button
          >
        {/if}
      </p>
    {/if}
    <!-- Beside the rail the Conversation scrolls by itself, as the diff does, and takes the focus for its
         keys (axe's scrollable-region-focusable). The Detail pane's scroll keys move it, which
         `data-scrolls` marks it for. -->
    <div
      class="body"
      style:--rail-width={`${String(panes.rail ?? RAIL_WIDTH)}px`}
      style:--beside-rail-least={`${String(BESIDE_RAIL_LEAST)}px`}
    >
      <div
        bind:this={panel}
        role="tabpanel"
        id="pr-tab-panel"
        aria-labelledby={`pr-tab-${pr.tab}`}
        tabindex={conversationScrolls ? 0 : undefined}
        data-scrolls={conversationScrolls ? "" : undefined}
      >
        {#if pr.tab === "conversation"}
          {#if view.content}
            <Conversation
              content={view.content}
              {now}
              {onperson}
              onlink={(address) => void links.openLink(address)}
              onthread={(id) => {
                pr.openThread(id);
              }}
            />
          {:else}
            <p class="byline">{PR_UNREAD[pr.failedRead ?? "reading"]}</p>
          {/if}
        {:else}
          <FilesTab {onperson} files={pr.files} {now} onlink={(address) => void links.openLink(address)} />
        {/if}
      </div>
      {#if railed}
        <Rail
          tabs={railTabs}
          rail={panes.railState}
          onleave={() => {
            focusBesideRail(section, panes.railState.tab);
          }}
        >
          {#snippet edge()}
            <PaneEdge
              label="Rail width"
              sizes={{ kind: "before", value: panes.rail ?? RAIL_WIDTH, previousLeast: BESIDE_RAIL_LEAST }}
              least={RAIL_LEAST}
              most={RAIL_MOST}
              onresize={(width) => (panes.rail = width)}
              ondone={() => void panes.save()}
              onreset={() => void panes.reset("rail")}
            />
          {/snippet}
        </Rail>
      {/if}
    </div>
  {/if}
</section>

<style>
  /* The diff scrolls by itself, and so do the Conversation and the rail beside each other, so the pane
     doesn't. */
  .detail-pane.files,
  .detail-pane.railed {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* The tab and the rail on its right fill the pane under the tab row. */
  .detail-pane.files .body,
  .detail-pane.railed .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  /* The tab keeps some width beside the rail, which gives way first. The width counts the Conversation's
     padding, as the rail's edge measures it. */
  .body > [role="tabpanel"] {
    flex: 1;
    box-sizing: border-box;
    min-width: var(--beside-rail-least);
  }

  /* The file view covers the tab, and stacks over nothing outside it. */
  .detail-pane.files .body > [role="tabpanel"] {
    position: relative;
    isolation: isolate;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .detail-pane.railed:not(.files) .body > [role="tabpanel"] {
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* The header and the Conversation sit in from the pane's edges; the diff in Files keeps its full width. */
  [role="alert"],
  .crumb,
  h2,
  h2 + .byline,
  .tab-row,
  .stale,
  .detail-pane:not(.files) [role="tabpanel"] {
    padding-inline: 16px;
  }

  .detail-pane:not(.files) [role="tabpanel"] {
    padding-bottom: 24px;
  }

  .crumb {
    display: flex;
    gap: 4px;
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .back {
    color: var(--accent);
  }

  h2 {
    margin: 4px 0 2px;
    font-size: 18px;
  }

  /* The open tab underlined in the accent colour, on a line across the pane, with the rail's button at the
     right. */
  .tab-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 8px 0 0;
    border-bottom: 1px solid var(--border);
  }

  .tabs {
    display: flex;
    flex: 1;
    gap: 2px;
  }

  .rail-button {
    all: unset;
    cursor: pointer;
    padding: 2px 8px;
    font-size: 12px;
    border-radius: 4px;
  }

  .rail-button:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  .rail-button[aria-pressed="true"] {
    background: var(--selected);
  }

  .rail-button:focus-visible {
    outline: 1px solid var(--focus-border);
  }

  /* Windows' contrast themes drop the tint, so the button says the rail is open with a border. */
  @media (forced-colors: active) {
    .rail-button[aria-pressed="true"] {
      border: 1px solid Highlight;
    }
  }

  /* window.css draws the tabs. */
  [role="tab"] {
    padding: 6px 12px;
  }

  /* One line under the tabs, on either tab, while the user's Review is a Stale review. */
  .stale {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin: 0;
    padding-block: 6px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }

  .stale span {
    overflow: hidden;
    color: var(--danger);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stale button {
    flex: none;
  }
</style>
