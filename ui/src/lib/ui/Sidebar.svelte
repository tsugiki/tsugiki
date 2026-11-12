<script lang="ts">
  import type { FilterLine, WorkbenchView } from "$lib/state/workbench.svelte";

  let {
    repository,
    onpick,
    count,
    reviewRequests,
    filters,
    selected,
    onselect,
  }: {
    /** The open Repository's name. */
    repository: string;
    /** Opens the Repository picker. */
    onpick: () => void;
    count: number;
    reviewRequests: number;
    filters: FilterLine[];
    selected: WorkbenchView;
    onselect: (view: WorkbenchView) => void;
  } = $props();

  const shown = (isShown: boolean) => (isShown ? "true" : undefined);
</script>

<nav aria-label="Sidebar">
  <button class="repository" aria-haspopup="dialog" onclick={onpick}>{repository}</button>
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
  /* A step off the content, with a border, as VS Code draws its side bar (#374). */
  nav {
    display: flex;
    flex: none;
    flex-direction: column;
    align-self: stretch;
    gap: 2px;
    /* The edge measures the whole box, padding and border too, and gives that width back. */
    box-sizing: border-box;
    width: var(--sidebar-width, 200px);
    padding: 6px;
    border-right: 1px solid var(--border);
    color: var(--fg-chrome);
    background: var(--bg-chrome);
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

  .repository {
    /* Not flex like the other lines: the ellipsis shows only on a block's own text. */
    display: block;
    margin-bottom: 6px;
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
