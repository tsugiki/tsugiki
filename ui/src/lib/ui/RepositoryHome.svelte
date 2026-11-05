<script lang="ts">
  import { WorkbenchState, type BackportRow, type ListedItem } from "$lib/state/workbench.svelte";
  import Sidebar from "./Sidebar.svelte";
  import { details, headline, issueDetails, preview, standaloneDetails } from "./workbench-row";

  const MINUTE = 60_000;
  const EMPTY = {
    whole: "Nothing here yet. The first sync lists what waits on you as it finds it.",
    filter:
      "Nothing in this Filter yet. If a sync has run, check its query in settings.toml: GitHub finds nothing for a search it can't read.",
    review_requests: "Nobody has asked you for a review.",
  };

  let { name, onadd }: { name: string; onadd: () => void } = $props();

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
    };
  });
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
    {#if item.attention}
      <strong>{headline(item)}</strong>
      <small class="preview">{preview(item.attention)}</small>
    {:else}
      <span>{headline(item)}</span>
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

<div class="home">
  <Sidebar
    count={workbench.count}
    reviewRequests={workbench.reviewRequests}
    filters={workbench.filters}
    selected={workbench.selected}
    onselect={(view) => void workbench.select(view)}
  />
  <section>
    <h1>{name}</h1>
    <button onclick={onadd}>Add repository</button>
    {#if workbench.error}
      <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
      <p role="alert">{`Tsugiki can't read this Workbench: ${workbench.error}`}</p>
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
</div>

<style>
  .home {
    display: flex;
    gap: 16px;
  }

  h1 {
    font-size: 16px;
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
