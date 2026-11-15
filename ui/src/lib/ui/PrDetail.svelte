<script lang="ts">
  import { type Snippet, untrack } from "svelte";
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { ReadFailure } from "$lib/state/detail-view.svelte";
  import {
    BESIDE_RAIL_LEAST,
    type PanesState,
    RAIL_LEAST,
    RAIL_MOST,
    RAIL_WIDTH,
  } from "$lib/state/panes.svelte";
  import type { PrTab, PrViewState } from "$lib/state/pr-view.svelte";
  import { RAIL_TABS, type RailTab, railTabLabel, type RailTabId } from "$lib/state/rail.svelte";
  import Conversation from "./Conversation.svelte";
  import FilesTab from "./FilesTab.svelte";
  import { focusBesideRail } from "./focus";
  import { currentOs, withKey } from "./keys";
  import MarkButton from "./MarkButton.svelte";
  import { prIcon } from "./octicons";
  import PaneEdge from "./PaneEdge.svelte";
  import Rail from "./Rail.svelte";
  import StateIcon from "./StateIcon.svelte";
  import StateWords from "./StateWords.svelte";
  import { ago, headline, staleReviewLine } from "./workbench-row";

  let {
    repository,
    pr,
    panes,
    now,
    onback,
    onmark,
    focusable = false,
    header,
  }: {
    repository: string;
    pr: PrViewState;
    /** The layout of the panes, which says whether the rail shows and how wide. */
    panes: PanesState;
    now: Date;
    onback: (issue: number) => void;
    /** Mark Seen or Mark Unseen, whichever the header's button offers. */
    onmark: () => void;
    /** Whether the pane takes the focus, as it does while the Issue list is hidden. Otherwise a click in it
     * leaves the focus where it was, so Enter still opens the row under the cursor. */
    focusable?: boolean;
    /** What sits at the top of the pane, such as the history's arrows (#586). */
    header?: Snippet;
  } = $props();

  const TABS: { tab: PrTab; name: string }[] = [
    { tab: "conversation", name: "Conversation" },
    { tab: "files", name: "Files" },
  ];

  /** The Conversation tab's words before its first read lands, and why it hasn't once a read failed. */
  const UNREAD: Record<ReadFailure | "reading", string> = {
    reading: "Reading from GitHub…",
    no_token:
      "Tsugiki can't ask GitHub for the Conversation without a token. The status bar says how to sign in.",
    waits_on_github: "The Conversation comes once GitHub answers.",
    not_on_github: "GitHub doesn't have this PR, or your token can't see it.",
    other: "Tsugiki couldn't read this PR's Conversation from GitHub. tsugiki.log says why.",
  };

  let browserProblem = $state<string>();
  // Svelte sets each to null as its element goes.
  let section = $state<HTMLElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  /** The rail's tabs, each with the pane it draws. */
  const railTabs: Readonly<Record<RailTabId, RailTab>> = { files: { ...RAIL_TABS.files, pane: filesPane } };
  /** Whether the rail shows: it opens beside the tabs, which come with the PR's view. */
  const railed = $derived(panes.railState.open && Boolean(pr.view));
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
  // Files button: on the page, the arrows would move the Issue list's cursor.
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

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
</script>

<!-- The rail's Files tab, which the tree of the PR's changed files fills (#604). -->
{#snippet filesPane()}{/snippet}

<section
  bind:this={section}
  class="detail"
  class:files={pr.tab === "files"}
  class:railed
  aria-label="PR view"
  tabindex={focusable ? -1 : undefined}
  data-pane="detail"
  onfocus={handOnFocus}
>
  {@render header?.()}
  {#if pr.error}
    <p role="alert">{`Tsugiki can't show this PR: ${pr.error}`}</p>
  {/if}
  {#if browserProblem}
    <p role="alert">{browserProblem}</p>
  {/if}
  <!-- The line over the title: the Issue the PR was opened under, where it goes and when it changed. -->
  {#if pr.issue !== undefined || pr.view}
    <p class="crumb">
      {#if pr.issue !== undefined}
        {@const issue = pr.issue}
        <button
          class="link back"
          onclick={() => {
            onback(issue);
          }}>{`gh-${String(issue)}`}</button
        >
        <span aria-hidden="true">›</span>
      {/if}
      {#if pr.view}
        <span>{`into ${pr.view.target_branch} · updated ${ago(pr.view.updated_at, now)}`}</span>
      {/if}
    </p>
  {/if}
  {#if pr.view === null}
    <p>{`Tsugiki doesn't hold PR #${String(pr.number)}.`}</p>
  {:else if pr.view}
    {@const view = pr.view}
    {@const stale = staleReviewLine(view.your_review, view.shares_review_of)}
    <h2>
      <StateIcon icon={prIcon(view)} />
      <button class="link" onclick={() => void onGitHub({ kind: "pull_request", number: view.number })}>
        {headline(view)}
      </button>
    </h2>
    <p class="by"><b>{view.author ?? "deleted user"}</b> <StateWords row={view} /></p>
    <MarkButton seen={view.seen} {onmark} />

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
          aria-label={`${label} in the rail`}
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
         keys (axe's scrollable-region-focusable). The Detail pane's ↑ and ↓ scroll it, which `data-scrolls`
         marks it for. -->
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
              onlink={(address) => void followLink(address)}
              onthread={(id) => {
                pr.openThread(id);
              }}
            />
          {:else}
            <p class="by">{UNREAD[pr.failedRead ?? "reading"]}</p>
          {/if}
        {:else}
          <FilesTab files={pr.files} {now} onlink={(address) => void followLink(address)} />
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
  /* Takes the window's height and scrolls by itself, as each pane does, and the wheel stops at its end. */
  .detail {
    flex: 1;
    min-width: 320px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* The diff scrolls by itself, and so do the Conversation and the rail beside each other, so the pane
     doesn't. */
  .detail.files,
  .detail.railed {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  /* The tab and the rail on its right fill the pane under the tab row. */
  .detail.files .body,
  .detail.railed .body {
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
  .detail.files .body > [role="tabpanel"] {
    position: relative;
    isolation: isolate;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .detail.railed:not(.files) .body > [role="tabpanel"] {
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  /* The header and the Conversation sit in from the pane's edges; the diff in Files keeps its full width. */
  [role="alert"],
  .crumb,
  h2,
  h2 + .by,
  .tab-row,
  .stale,
  .detail:not(.files) [role="tabpanel"] {
    padding-inline: 16px;
  }

  .detail:not(.files) [role="tabpanel"] {
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

  .by {
    margin: 0 0 8px;
    font-size: 13px;
    color: var(--fg-muted);
  }

  .by b {
    font-weight: 600;
    color: var(--fg);
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

  [role="tab"] {
    all: unset;
    cursor: pointer;
    padding: 6px 12px;
    font-size: 12px;
    border-bottom: 2px solid transparent;
  }

  [role="tab"][aria-selected="true"] {
    font-weight: 600;
    border-bottom-color: var(--accent);
  }

  [role="tab"]:focus-visible {
    outline: 1px solid var(--focus-border);
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
