<!-- The search in the diff (#186), which Ctrl+F opens over the Files tab. It finds text in every row, those
     the tab doesn't hold too, and says which match the tab is on. Enter and Shift+Enter move between the
     matches, and Escape closes it. -->
<script lang="ts">
  import type { PrFilesState } from "$lib/state/pr-files.svelte";

  let { files }: { files: PrFilesState } = $props();

  /** The match the tab is on and how many there are, or that there are none, once the core answers, or why
   * it couldn't search. */
  const status = $derived.by(() => {
    if (files.searchProblem !== undefined) return `Can't search: ${files.searchProblem}`;
    const search = files.search;
    if (search === undefined) return "";
    if (search.at === undefined) return "No results";
    return `${String(search.at + 1)} of ${String(search.count)}${search.more ? "+" : ""}`;
  });

  /** Gives the box focus with its text selected as Ctrl+F opens it, and again on each Ctrl+F. A tab drawn
   * again with the search open leaves the focus where it is. */
  const focused = (box: HTMLInputElement) => {
    if (!files.searchWantsFocus) return;
    files.searchWantsFocus = false;
    box.focus();
    box.select();
  };
</script>

<div class="find" role="search" aria-label="Find in the diff">
  <input
    type="text"
    aria-label="Find"
    placeholder="Find"
    value={files.query}
    {@attach focused}
    oninput={(event) => void files.find(event.currentTarget.value)}
    onkeydown={(event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        files.nextMatch(event.shiftKey ? -1 : 1);
      } else if (event.key === "Escape") {
        event.preventDefault();
        files.closeSearch();
      }
    }}
  />
  <span class="count" aria-live="polite">{status}</span>
  <button
    type="button"
    aria-label="Previous match"
    title="Previous match (Shift+Enter)"
    disabled={files.search?.at === undefined}
    onclick={() => {
      files.nextMatch(-1);
    }}>↑</button
  >
  <button
    type="button"
    aria-label="Next match"
    title="Next match (Enter)"
    disabled={files.search?.at === undefined}
    onclick={() => {
      files.nextMatch(1);
    }}>↓</button
  >
  <button
    type="button"
    aria-label="Close"
    title="Close (Escape)"
    onclick={() => {
      files.closeSearch();
    }}>×</button
  >
</div>

<style>
  .find {
    display: flex;
    gap: 4px;
    align-items: center;
    padding: 2px 0;
    font-size: 12px;
  }

  input {
    width: 24ch;
    font: inherit;
  }

  .count {
    min-width: 10ch;
    color: var(--fg-muted);
  }

  button {
    border: 0;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
  }

  button:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
