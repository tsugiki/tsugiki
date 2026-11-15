<script lang="ts">
  import { tick, type Snippet } from "svelte";
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
  import Conversation from "./Conversation.svelte";
  import FilesTab from "./FilesTab.svelte";
  import { focusBesideRail } from "./focus";
  import { actionLabel, currentOs } from "./keys";
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
  let section = $state<HTMLElement>();
  /** The rail's key, which its button and × name. */
  const railKey = actionLabel("toggle_rail_files", currentOs());

  /** × in the rail: closes it, and gives the focus, which was on ×, to what the rail sat beside, or else to
   * the Files button. */
  async function closeRail() {
    panes.railOpen = false;
    await tick();
    focusBesideRail(section);
  }

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
</script>

<section
  bind:this={section}
  class="detail"
  class:files={pr.tab === "files"}
  class:railed={panes.railOpen}
  aria-label="PR view"
  tabindex={focusable ? -1 : undefined}
  data-pane="detail"
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

    <!-- The tabs, and at the right the button that opens the rail beside them, or closes it. -->
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
      <button
        type="button"
        class="rail-button"
        data-opens-rail
        aria-pressed={panes.railOpen}
        title={`Files in the rail (${railKey})`}
        onclick={() => {
          panes.railOpen = !panes.railOpen;
        }}>Files</button
      >
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
    {@const scrolls = panes.railOpen && pr.tab === "conversation"}
    <div class="body" style:--rail-width={`${String(panes.rail ?? RAIL_WIDTH)}px`}>
      <div
        role="tabpanel"
        id="pr-tab-panel"
        aria-labelledby={`pr-tab-${pr.tab}`}
        tabindex={scrolls ? 0 : undefined}
        data-scrolls={scrolls ? "" : undefined}
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
      {#if panes.railOpen}
        <PaneEdge
          label="Rail width"
          sizes={{ kind: "before", value: panes.rail ?? RAIL_WIDTH, previousLeast: BESIDE_RAIL_LEAST }}
          least={RAIL_LEAST}
          most={RAIL_MOST}
          onresize={(width) => (panes.rail = width)}
          ondone={() => void panes.save()}
          onreset={() => void panes.reset("rail")}
        />
        <Rail
          closeKey={railKey}
          onclose={() => void closeRail()}
          onleave={() => {
            focusBesideRail(section);
          }}
        />
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

  /* The tab keeps some width beside the rail, which gives way first. */
  .body > [role="tabpanel"] {
    flex: 1;
    min-width: 240px;
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
