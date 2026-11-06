<script lang="ts">
  import { openOnGitHub, type GitHubItem } from "$lib/state/browser";
  import { hud } from "$lib/state/hud.svelte";
  import { IssueViewState } from "$lib/state/issue-view.svelte";
  import { WorkbenchState, type BackportRow, type ListedItem } from "$lib/state/workbench.svelte";
  import IssueDetail from "./IssueDetail.svelte";
  import { currentOs, isTextBox, keyName } from "./keys";
  import Sidebar from "./Sidebar.svelte";
  import { details, headline, issueDetails, preview, standaloneDetails } from "./workbench-row";

  const MINUTE = 60_000;
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
  $effect(() => {
    // By the time this cleans up, `workbench` is already the next Repository's.
    const opened = workbench;
    void opened.load();
    return () => {
      opened.stop();
      issue?.stop();
      issue = undefined;
    };
  });

  // The first Workbench drawn ends the start-up the performance HUD times. Effects run once the rows are in
  // the page.
  $effect(() => {
    if (workbench.items !== undefined) void hud.markUsable();
  });

  /** The Issue in the Detail pane. */
  let issue = $state<IssueViewState>();
  let browserProblem = $state<string>();
  let list = $state<HTMLElement>();

  const onGitHub = async (item: GitHubItem) => {
    browserProblem = await openOnGitHub(name, item);
  };

  /** Shows an Issue in the Detail pane. A PR has no view of its own until stage 2, so it opens on GitHub. */
  function open(item: ListedItem) {
    workbench.cursor = item.number;
    if (item.kind !== "issue") {
      void onGitHub({ kind: "pull_request", number: item.number });
    } else if (issue?.number === item.number) {
      void issue.open();
    } else {
      issue?.stop();
      issue = new IssueViewState(name, item.number);
      void issue.load();
    }
  }

  /** The Issue in the Detail pane, or else the item under the cursor. */
  function currentOnGitHub(): GitHubItem | undefined {
    if (issue) return { kind: "issue", number: issue.number };
    const item = workbench.underCursor;
    if (!item) return undefined;
    return { kind: item.kind === "issue" ? "issue" : "pull_request", number: item.number };
  }

  function moveCursor(step: 1 | -1) {
    workbench.moveCursor(step);
    // Focus follows, so the row scrolls into view, and Enter presses its button.
    list?.querySelector<HTMLElement>(`[data-item="${String(workbench.cursor)}"]`)?.focus();
  }

  /**
   * The list's keys. A dialog on top keeps them, and a text box or a focused control keeps its own. The
   * window's guard has already stopped the browser for Ctrl+Shift+G.
   */
  function onkeydown(e: KeyboardEvent) {
    if (!keysActive) return;
    const key = keyName(e, currentOs());
    if (key === "ctrl+shift+g") {
      const current = currentOnGitHub();
      if (current) void onGitHub(current);
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
    }
  }
</script>

{#snippet backports(rows: BackportRow[])}
  {#if rows.length > 0}
    <ul class="pull-requests">
      {#each rows as row (row.number)}
        <li>
          <span>{headline(row)}</span>
          <small>{details(row)}</small>
        </li>
      {/each}
    </ul>
  {/if}
{/snippet}

{#snippet listed(item: ListedItem)}
  <li>
    <button
      class="open"
      data-item={item.number}
      aria-current={workbench.cursor === item.number ? "true" : undefined}
      onclick={() => {
        open(item);
      }}
    >
      {#if item.attention}
        <strong>{headline(item)}</strong>
      {:else}
        <span>{headline(item)}</span>
      {/if}
    </button>
    {#if item.attention}
      <small class="preview">{preview(item.attention)}</small>
    {/if}
    {#if item.kind === "issue"}
      <small>{issueDetails(item, now)}</small>
      <ul class="pull-requests">
        {#each item.pull_requests as entry (entry.number)}
          <li>
            <span>{headline(entry)}</span>
            <small>{details(entry)}</small>
            {#if entry.kind === "pr"}
              {@render backports(entry.backports)}
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <small>{standaloneDetails(item, now)}</small>
      {#if item.kind === "standalone_pr"}
        {@render backports(item.backports)}
      {/if}
    {/if}
  </li>
{/snippet}

<svelte:window {onkeydown} />

<div class="home">
  <Sidebar
    count={workbench.count}
    reviewRequests={workbench.reviewRequests}
    filters={workbench.filters}
    selected={workbench.selected}
    onselect={(view) => void workbench.select(view)}
  />
  <section class="list" bind:this={list}>
    <h1>{name}</h1>
    <button onclick={onadd}>Add repository</button>
    {#if workbench.error}
      <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
      <p role="alert">{`Tsugiki can't read this Workbench: ${workbench.error}`}</p>
    {/if}
    {#if browserProblem}
      <p role="alert">{browserProblem}</p>
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
  {#if issue}
    {#key issue}
      <IssueDetail repository={name} {issue} {now} />
    {/key}
  {/if}
</div>

<style>
  .home {
    display: flex;
    gap: 16px;
  }

  .list {
    flex: 1;
    min-width: 280px;
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
