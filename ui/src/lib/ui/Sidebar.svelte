<script lang="ts">
  import type { FilterLine, WorkbenchView } from "$lib/state/workbench.svelte";

  let {
    count,
    reviewRequests,
    filters,
    selected,
    onselect,
  }: {
    count: number;
    reviewRequests: number;
    filters: FilterLine[];
    selected: WorkbenchView;
    onselect: (view: WorkbenchView) => void;
  } = $props();

  const shown = (isShown: boolean) => (isShown ? "true" : undefined);
</script>

<nav aria-label="Sidebar">
  <button aria-current={shown(selected.kind === "whole")} onclick={() => onselect({ kind: "whole" })}>
    <span>Workbench</span>
    <span>{count}</span>
  </button>
  <button
    aria-current={shown(selected.kind === "review_requests")}
    onclick={() => onselect({ kind: "review_requests" })}
  >
    <span>Review requests</span>
    <span>{reviewRequests}</span>
  </button>
  <!-- Two Filters may share a search, so a search can't be the key. -->
  {#each filters as filter, index (index)}
    <button
      aria-current={shown(selected.kind === "filter" && selected.query === filter.query)}
      onclick={() => onselect({ kind: "filter", query: filter.query })}
    >
      <span>{filter.name}</span>
      <span>{filter.count}</span>
    </button>
    {#if filter.problem}
      <small>{filter.problem}</small>
    {/if}
  {/each}
</nav>

<style>
  nav {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 2px;
    width: 200px;
  }

  button {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    text-align: left;
  }

  [aria-current] {
    font-weight: 600;
  }
</style>
