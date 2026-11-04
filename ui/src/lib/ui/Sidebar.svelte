<script lang="ts">
  import type { FilterLine } from "$lib/state/workbench.svelte";

  let {
    count,
    filters,
    selected,
    onselect,
  }: {
    count: number;
    filters: FilterLine[];
    selected: string | null;
    onselect: (query: string | null) => void;
  } = $props();
</script>

<nav aria-label="Sidebar">
  <button aria-current={selected === null ? "true" : undefined} onclick={() => onselect(null)}>
    <span>Workbench</span>
    <span>{count}</span>
  </button>
  <!-- Two Filters may share a search, so a search can't be the key. -->
  {#each filters as filter, index (index)}
    <button
      aria-current={selected === filter.query ? "true" : undefined}
      onclick={() => onselect(filter.query)}
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
