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
  <h2 class="small-heading">Built-in views</h2>
  <button
    aria-current={shown(selected.kind === "review_requests")}
    onclick={() => onselect({ kind: "review_requests" })}
  >
    <span>Review requests</span>
    <span>{reviewRequests}</span>
  </button>
  {#if filters.length > 0}
    <h2 class="small-heading">Filters</h2>
  {/if}
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
    gap: 1px;
    /* The edge measures the whole box, padding and border too, and gives that width back. */
    box-sizing: border-box;
    width: var(--sidebar-width, 200px);
    padding: 6px;
    border-right: 1px solid var(--border);
    font-size: 13px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
  }

  /* An entry: a line of text with its count at the right, tinted under the pointer. */
  button {
    all: unset;
    display: flex;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    cursor: pointer;
  }

  button:hover {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  button:focus-visible {
    outline: 1px solid var(--accent);
  }

  button > span + span {
    color: var(--fg-muted);
  }

  [aria-current] {
    font-weight: 600;
    background: var(--selected);
  }

  [aria-current] > span + span {
    font-weight: 400;
  }

  .repository {
    /* Not flex like the other lines: the ellipsis shows only on a block's own text. */
    display: block;
    margin-bottom: 4px;
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  h2 {
    margin: 0;
    padding: 12px 8px 4px;
  }

  /* What went wrong with the Filter above. */
  small {
    padding: 0 8px 4px;
  }
</style>
