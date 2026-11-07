<!-- The rows of a PR's diff, drawn only near what the tab shows. Every row has the same height, so a row's
     place follows from its number, and the core sends the rows in chunks as they come near (ADR 0001). -->
<script lang="ts">
  import type { DiffFile, FileChange, PrFilesState } from "$lib/state/pr-files.svelte";
  import { pieces } from "./spans";

  let { diff, rows, files }: { diff: PrFilesState; rows: number; files: DiffFile[] } = $props();

  /** A row's height in pixels. */
  const ROW = 20;
  /** Rows drawn above and below the visible ones, so a quick scroll doesn't show blanks. */
  const OVERSCAN = 40;

  let scrollTop = $state(0);
  let height = $state(0);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows - 1, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const shown = $derived(Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i));
  /** Room for the longest line, so the scroll width doesn't change as rows come and go. */
  const columns = $derived(files.reduce((widest, file) => Math.max(widest, file.columns), 0));

  $effect(() => {
    diff.need(first, last);
  });

  const counted = (change: FileChange) =>
    change.kind === "binary" ? "binary" : `+${String(change.added)} −${String(change.removed)}`;
  const named = (file: { path: string; old_path: string | null }) =>
    file.old_path === null ? file.path : `${file.old_path} → ${file.path}`;
</script>

<!-- A line of code after its marker, highlighted once its spans come. Each piece sits on one line, as the
     row keeps its text's spaces. -->
<!-- prettier-ignore -->
{#snippet code(marker: string, text: string, index: number)}<span class="text">{marker}{#each pieces(text, diff.spans(index) ?? []) as piece, at (at)}{#if piece.highlight}<span class={`hl-${piece.highlight}`}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span>{/snippet}

<!-- A region that scrolls takes focus, so the keys scroll it (axe's scrollable-region-focusable). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="scroller"
  role="region"
  aria-label="Diff"
  tabindex="0"
  bind:clientHeight={height}
  onscroll={(event) => {
    scrollTop = event.currentTarget.scrollTop;
  }}
>
  <div
    class="spacer"
    style:height={`${String(rows * ROW)}px`}
    style:min-width={`calc(${String(columns)}ch + 112px)`}
  >
    {#each shown as index (index)}
      {@const row = diff.row(index)}
      <div class={`row ${row?.kind ?? "waiting"}`} style:top={`${String(index * ROW)}px`}>
        {#if row?.kind === "file"}
          <span class="path">{named(row)}</span><span class="count">{counted(row.change)}</span>
        {:else if row?.kind === "hunk" || row?.kind === "note"}
          <span class="number"></span><span class="number"></span><span class="text">{row.text}</span>
        {:else if row?.kind === "context"}
          <span class="number">{row.old}</span><span class="number">{row.new}</span>{@render code(
            " ",
            row.text,
            index,
          )}
        {:else if row?.kind === "added"}
          <span class="number"></span><span class="number">{row.new}</span>{@render code(
            "+",
            row.text,
            index,
          )}
        {:else if row?.kind === "removed"}
          <span class="number">{row.old}</span><span class="number"></span>{@render code(
            "-",
            row.text,
            index,
          )}
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
  }

  .scroller:focus-visible {
    outline: 1px solid currentColor;
  }

  .spacer {
    position: relative;
  }

  .row {
    position: absolute;
    left: 0;
    right: 0;
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

  .file {
    gap: 12px;
    padding-left: 8px;
    font-weight: 600;
    border-top: 1px solid color-mix(in srgb, currentColor 20%, transparent);
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  .count {
    font-weight: normal;
    opacity: 0.7;
  }

  .hunk {
    background: color-mix(in srgb, #0969da 10%, transparent);
    opacity: 0.8;
  }

  .note {
    font-style: italic;
    opacity: 0.7;
  }

  .added {
    background: color-mix(in srgb, #2da44e 18%, transparent);
  }

  .removed {
    background: color-mix(in srgb, #cf222e 18%, transparent);
  }

  /* github.com's colors for code. The classes come from the spans, which Svelte can't see, so they're global
     within a row's text. */
  .text :global(.hl-keyword) {
    color: var(--hl-keyword);
  }

  .text :global(.hl-string) {
    color: var(--hl-string);
  }

  .text :global(.hl-comment) {
    color: var(--hl-comment);
  }

  .text :global(.hl-constant) {
    color: var(--hl-constant);
  }

  .text :global(.hl-function),
  .text :global(.hl-type) {
    color: var(--hl-entity);
  }

  /* github.com's light palette. The window has no dark theme yet: one that sets `data-theme="dark"` on the
     root gets github.com's dark palette below. */
  .scroller {
    --hl-keyword: #cf222e;
    --hl-string: #0a3069;
    --hl-comment: #59636e;
    --hl-constant: #0550ae;
    --hl-entity: #6639ba;
  }

  :global(:root[data-theme="dark"]) .scroller {
    --hl-keyword: #ff7b72;
    --hl-string: #a5d6ff;
    --hl-comment: #9198a1;
    --hl-constant: #79c0ff;
    --hl-entity: #d2a8ff;
  }
</style>
