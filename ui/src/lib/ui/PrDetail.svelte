<script lang="ts">
  import { openLink, openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import type { PrTab, PrViewState } from "$lib/state/pr-view.svelte";
  import Conversation from "./Conversation.svelte";
  import FilesTab from "./FilesTab.svelte";
  import { headline } from "./workbench-row";

  let {
    repository,
    pr,
    now,
    onback,
  }: { repository: string; pr: PrViewState; now: Date; onback: () => void } = $props();

  const TABS: { tab: PrTab; name: string }[] = [
    { tab: "conversation", name: "Conversation" },
    { tab: "files", name: "Files" },
  ];

  let browserProblem = $state<string>();

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(repository, item);
  };
  const followLink = async (address: string) => {
    browserProblem = await openLink(address);
  };
</script>

<section class="detail" class:files={pr.tab === "files"} aria-label="PR view">
  {#if pr.error}
    <p role="alert">{`Tsugiki can't show this PR: ${pr.error}`}</p>
  {/if}
  {#if browserProblem}
    <p role="alert">{browserProblem}</p>
  {/if}
  {#if pr.issue !== undefined}
    <button class="link back" onclick={onback}>{`gh-${String(pr.issue)}`}</button>
  {/if}
  {#if pr.view === null}
    <p>{`Tsugiki doesn't hold PR #${String(pr.number)}.`}</p>
  {:else if pr.view}
    {@const view = pr.view}
    <h2>
      <button class="link" onclick={() => void onGitHub({ kind: "pull_request", number: view.number })}>
        {headline(view)}
      </button>
    </h2>
    <p class="by">{view.author ?? "deleted user"}</p>

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
          <p class="by">Reading from GitHub…</p>
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

  h2 {
    font-size: 16px;
    margin: 0;
  }

  .by {
    font-size: 12px;
    margin: 4px 0;
    opacity: 0.75;
  }

  .link {
    all: unset;
    cursor: pointer;
  }

  .link:hover,
  .link:focus-visible {
    text-decoration: underline;
  }

  .back {
    font-size: 12px;
  }

  .tabs {
    display: flex;
    gap: 4px;
    margin: 12px 0;
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  [role="tab"] {
    all: unset;
    cursor: pointer;
    padding: 4px 8px;
  }

  [role="tab"][aria-selected="true"] {
    border-bottom: 2px solid currentColor;
  }

  [role="tab"]:focus-visible {
    outline: 1px solid currentColor;
  }
</style>
