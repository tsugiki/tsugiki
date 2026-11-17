<!-- The code an Outdated thread was written against (#185): the rows of the hunk GitHub stored with it,
     highlighted, ending at the thread's line. The diff's scroller colors the highlighted code. -->
<script lang="ts">
  import type { CodeRow, Span } from "$lib/state/pr-files.svelte";
  import CodePieces from "./CodePieces.svelte";

  let { rows, spans }: { rows: CodeRow[]; spans: Span[][] } = $props();

  /** A row's line numbers, its marker and its text, or for the `@@` line and a note, its text alone. */
  const parts = (row: CodeRow) => {
    switch (row.kind) {
      case "context":
        return { old: row.old, new: row.new, marker: " ", text: row.text };
      case "added":
        return { old: null, new: row.new, marker: "+", text: row.text };
      case "removed":
        return { old: row.old, new: null, marker: "-", text: row.text };
      case "hunk":
      case "note":
        return { old: null, new: null, marker: "", text: row.text };
    }
  };
</script>

<div class="code mono">
  {#each rows as row, at (at)}
    {@const shown = parts(row)}
    <!-- prettier-ignore -->
    <div class={`row ${row.kind}`}><span class="number">{shown.old ?? ""}</span><span class="number">{shown.new ?? ""}</span><span class="text">{shown.marker}<CodePieces text={shown.text} spans={spans[at] ?? []} /></span></div>
  {/each}
</div>

<style>
  .code {
    margin: 0 8px 4px;
    overflow-x: auto;
    font-size: 12px;
    border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .hunk,
  .note {
    color: var(--fg-muted);
  }

  .hunk {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .added {
    background: color-mix(in srgb, var(--add) 18%, transparent);
  }

  .removed {
    background: color-mix(in srgb, var(--del) 18%, transparent);
  }
</style>
