<script lang="ts">
  import type { Snippet } from "svelte";
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { ReadFailure } from "$lib/state/detail-view.svelte";
  import type { PrTab, PrViewState } from "$lib/state/pr-view.svelte";
  import Conversation from "./Conversation.svelte";
  import FilesTab from "./FilesTab.svelte";
  import { prIcon } from "./octicons";
  import StateIcon from "./StateIcon.svelte";
  import StateWords from "./StateWords.svelte";
  import { ago, headline } from "./workbench-row";

  let {
    repository,
    pr,
    now,
    onback,
    focusable = false,
    header,
  }: {
    repository: string;
    pr: PrViewState;
    now: Date;
    onback: (issue: number) => void;
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

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
</script>

<section
  class="detail"
  class:files={pr.tab === "files"}
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
    <h2>
      <StateIcon icon={prIcon(view)} />
      <button class="link" onclick={() => void onGitHub({ kind: "pull_request", number: view.number })}>
        {headline(view)}
      </button>
    </h2>
    <p class="by"><b>{view.author ?? "deleted user"}</b> <StateWords row={view} /></p>

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
    <div role="tabpanel" id="pr-tab-panel" aria-labelledby={`pr-tab-${pr.tab}`}>
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
  {/if}
</section>

<style>
  /* Stays on screen beside a long list, and scrolls by itself, clear of the status bar. */
  .detail {
    flex: 1;
    min-width: 320px;
    position: sticky;
    top: 0;
    align-self: flex-start;
    max-height: calc(100vh - 48px);
    overflow-y: auto;
  }

  /* The diff scrolls by itself, so the pane takes the window's height and doesn't scroll. */
  .detail.files {
    display: flex;
    flex-direction: column;
    height: calc(100vh - 48px);
    overflow: hidden;
  }

  .detail.files [role="tabpanel"] {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  /* The header and the Conversation sit in from the pane's edges; the diff in Files keeps its full width. */
  [role="alert"],
  .crumb,
  h2,
  h2 + .by,
  .tabs,
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

  /* The open tab underlined in the accent colour, on a line across the pane. */
  .tabs {
    display: flex;
    gap: 2px;
    margin: 8px 0 0;
    border-bottom: 1px solid var(--border);
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
</style>
