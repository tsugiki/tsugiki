<!-- The code an Outdated thread was written against (#185): the rows of the hunk GitHub stored with it,
     highlighted, ending at the thread's line. The diff's scroller colors the highlighted code. -->
<script lang="ts">
  import type { CodeRow, Span } from "$lib/state/pr-files.svelte";
  import { pieces } from "./spans";

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

<div class="code">
  {#each rows as row, at (at)}
    {@const shown = parts(row)}
    <!-- prettier-ignore -->
    <div class={`row ${row.kind}`}><span class="number">{shown.old ?? ""}</span><span class="number">{shown.new ?? ""}</span><span class="text">{shown.marker}{#each pieces(shown.text, spans[at] ?? []) as piece, index (index)}{#if piece.highlight}<span class={`hl-${piece.highlight}`}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span></div>
  {/each}
</div>

<style>
  .code {
    margin: 0 8px 4px;
    overflow-x: auto;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }

  .row {
    display: flex;
    height: 20px;
    line-height: 20px;
    white-space: pre;
    tab-size: 8;
  }

  .number {
    flex: none;
    width: 48px;
    padding-right: 8px;
    box-sizing: border-box;
    text-align: right;
    opacity: 0.55;
    user-select: none;
  }

  .hunk,
  .note {
    opacity: 0.7;
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
