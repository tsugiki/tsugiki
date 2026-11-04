<script lang="ts">
  import { WorkbenchState } from "$lib/state/workbench.svelte";
  import { details, headline, issueDetails, standaloneDetails } from "./workbench-row";

  const MINUTE = 60_000;

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

<section>
  <h1>{name}</h1>
  <button onclick={onadd}>Add repository</button>
  {#if workbench.error}
    <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
    <p role="alert">{`Tsugiki can't read this Workbench: ${workbench.error}`}</p>
  {/if}
  {#if workbench.items?.length === 0}
    <p>Nothing here yet. The first sync lists what waits on you as it finds it.</p>
  {:else if workbench.items}
    <ul>
      {#each workbench.items as item (item.number)}
        <li>
          <span>{headline(item)}</span>
          {#if item.kind === "issue"}
            <small>{issueDetails(item, now)}</small>
            <ul class="pull-requests">
              {#each item.pull_requests as row (row.number)}
                <li>
                  <span>{headline(row)}</span>
                  <small>{details(row)}</small>
                </li>
              {/each}
            </ul>
          {:else}
            <small>{standaloneDetails(item, now)}</small>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  h1 {
    font-size: 16px;
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
