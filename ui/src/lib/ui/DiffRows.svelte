<!-- The rows of a PR's diff, drawn only near what the tab shows, below its NEWS entries. Every row has the
     same height, so a row's place follows from where it shows, and the core sends the rows in chunks as they
     come near (ADR 0001). A collapsed Generated file shows only its header (#181). A hunk's row offers to
     show the lines its gap leaves out, and the row after a file's last hunk the lines after it (#182).
     Side by side, a line of context shows on both sides, and a change shows its removed rows on the left
     beside its added ones on the right (#183). -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { DiffLayout, HiddenGap } from "$lib/state/diff-layout";
  import {
    EXPAND,
    type DiffFile,
    type FileChange,
    type NewsEntry,
    type PrFilesState,
    type Span,
  } from "$lib/state/pr-files.svelte";
  import NewsEntries from "./NewsEntries.svelte";
  import { pieces } from "./spans";

  let {
    diff,
    files,
    news,
    onlink,
  }: { diff: PrFilesState; files: DiffFile[]; news: NewsEntry[]; onlink: (address: string) => void } =
    $props();

  /** A row's height in pixels. */
  const ROW = 20;
  /** Rows drawn above and below the visible ones, so a quick scroll doesn't show blanks. */
  const OVERSCAN = 40;

  let scroller = $state<HTMLElement>();
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let height = $state(0);
  let width = $state(0);
  /** How tall the NEWS entries above the rows are. */
  let newsHeight = $state(0);
  const rows = $derived(diff.layout.count);
  const first = $derived(Math.max(0, Math.floor((scrollTop - newsHeight) / ROW) - OVERSCAN));
  const last = $derived(Math.min(rows - 1, Math.ceil((scrollTop + height - newsHeight) / ROW) + OVERSCAN));
  const shown = $derived(Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i));
  /** Room for the longest line, so the scroll width doesn't change as rows come and go. */
  const columns = $derived(files.reduce((widest, file) => Math.max(widest, file.columns), diff.columns));
  const sideBySide = $derived(diff.shownAs === "side-by-side");

  $effect(() => {
    diff.need(first, last);
  });

  /** The layout the rows were last drawn from, which a switch of view starts from. */
  let drawn: DiffLayout | undefined;
  /** Keeps the place at the top in view as the view switches: the same row, line or button, as far into
   * it as it was. */
  $effect(() => {
    const layout = diff.layout;
    const before = drawn;
    drawn = layout;
    untrack(() => {
      const into = scrollTop - newsHeight;
      if (before === undefined || before.view === layout.view || into <= 0 || scroller === undefined) return;
      scroller.scrollTop = newsHeight + layout.moved(before, Math.floor(into / ROW)) * ROW + (into % ROW);
      // The rows to draw follow at once, as the scroll event comes after the next frame.
      scrollTop = scroller.scrollTop;
    });
  });

  const counted = (change: FileChange) =>
    change.kind === "binary" ? "binary" : `+${String(change.added)} −${String(change.removed)}`;
  const named = (file: { path: string; old_path: string | null }) =>
    file.old_path === null ? file.path : `${file.old_path} → ${file.path}`;
  /** The place among the files of the Generated file whose header is row `index` of the diff, if it is
   * one. */
  const generated = (index: number) => {
    const at = diff.layout.fileAt(index);
    return at !== undefined && files[at]?.generated ? at : undefined;
  };
</script>

<!-- A line of code after its marker, highlighted once its spans come. Each piece sits on one line, as the
     row keeps its text's spaces. -->
<!-- prettier-ignore -->
{#snippet code(marker: string, text: string, spans: Span[])}<span class="text">{marker}{#each pieces(text, spans) as piece, at (at)}{#if piece.highlight}<span class={`hl-${piece.highlight}`}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span>{/snippet}

<!-- One side of a row side by side: a line's number and its code. -->
<!-- prettier-ignore -->
{#snippet side(kind: string, number: number, marker: string, text: string, spans: Span[])}<span class={`side ${kind}`}><span class="number">{number}</span><span class="clip">{@render code(marker, text, spans)}</span></span>{/snippet}

<!-- A side with nothing on it, as the right side of a removed row has, or one whose row hasn't come. -->
{#snippet blank(kind: string)}<span class={`side ${kind}`}></span>{/snippet}

<!-- A line of context side by side: its old number and its new, with its code on both sides. -->
{#snippet both(kind: string, old: number, line: number, text: string, spans: Span[])}
  {@render side(kind, old, " ", text, spans)}{@render side(kind, line, " ", text, spans)}
{/snippet}

<!-- Side by side, one side of a change: row `index` of the diff, removed or added, the note that the line
     above has no newline, or nothing. -->
{#snippet paired(index: number | null)}
  {@const row = index === null ? undefined : diff.row(index)}
  {#if index === null}
    {@render blank("")}
  {:else if row?.kind === "removed"}
    {@render side("removed", row.old, "-", row.text, diff.spans(index) ?? [])}
  {:else if row?.kind === "added"}
    {@render side("added", row.new, "+", row.text, diff.spans(index) ?? [])}
  {:else if row?.kind === "note"}
    <span class="side note"
      ><span class="number"></span><span class="clip"><span class="text">{row.text}</span></span></span
    >
  {:else}
    {@render blank("waiting")}
  {/if}
{/snippet}

<!-- The button that shows the whole file. -->
{#snippet whole(file: number)}
  <button
    type="button"
    class="whole"
    onclick={() => {
      void diff.expandFile(file);
    }}>Whole file</button
  >
{/snippet}

<!-- The buttons of a gap that still hides lines, in the place of a hunk row's line numbers: 20 more above the
     hunk or below the one before, or the rest of a short gap. As on GitHub, the gap at a file's start has
     no hunk before it, so it shows lines only above its hunk. -->
{#snippet gap({ file, gap, hidden }: HiddenGap)}
  <span class="expand">
    {#if hidden > EXPAND}
      <button
        type="button"
        class="step"
        aria-label={`Show ${String(EXPAND)} lines above`}
        title={`Show ${String(EXPAND)} lines above`}
        onclick={() => {
          diff.expandAbove(file, gap);
        }}>↑</button
      >{#if gap.new > 1}<button
          type="button"
          class="step"
          aria-label={`Show ${String(EXPAND)} lines below`}
          title={`Show ${String(EXPAND)} lines below`}
          onclick={() => {
            diff.expandBelow(file, gap);
          }}>↓</button
        >{/if}
    {:else}
      <button
        type="button"
        class="step"
        aria-label={`Show ${String(hidden)} hidden lines`}
        title={`Show ${String(hidden)} hidden lines`}
        onclick={() => {
          diff.expandGap(file, gap);
        }}>↕</button
      >
    {/if}
  </span>{@render whole(file)}
{/snippet}

<!-- A region that scrolls takes focus, so the keys scroll it (axe's scrollable-region-focusable). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="scroller"
  role="region"
  aria-label={sideBySide ? "Side-by-side diff" : "Diff"}
  tabindex="0"
  bind:this={scroller}
  bind:clientHeight={height}
  bind:clientWidth={width}
  onscroll={(event) => {
    scrollTop = event.currentTarget.scrollTop;
    scrollLeft = event.currentTarget.scrollLeft;
  }}
>
  <div bind:clientHeight={newsHeight}>
    <NewsEntries {news} {onlink} />
  </div>
  <div
    class="spacer"
    style:height={`${String(rows * ROW)}px`}
    style:min-width={sideBySide
      ? `calc(${String(width)}px + max(0px, ${String(columns + 1)}ch + 56px - ${String(width / 2)}px))`
      : `calc(${String(columns)}ch + 112px)`}
    style:--across={`${String(scrollLeft)}px`}
    style:--width={`${String(width)}px`}
  >
    {#each shown as place (place)}
      {@const at = diff.layout.at(place)}
      {#if at.kind === "line" && sideBySide}
        {@const line = diff.line(at.file, at.new)}
        <div class="row split" style:top={`${String(place * ROW)}px`}>
          {@render both("context expanded", at.old, at.new, line.text, line.spans)}
        </div>
      {:else if at.kind === "line"}
        {@const line = diff.line(at.file, at.new)}
        <div class="row context expanded" style:top={`${String(place * ROW)}px`}>
          <span class="number">{at.old}</span><span class="number">{at.new}</span>{@render code(
            " ",
            line.text,
            line.spans,
          )}
        </div>
      {:else if at.kind === "pair"}
        <div class="row split" style:top={`${String(place * ROW)}px`}>
          {@render paired(at.removed)}{@render paired(at.added)}
        </div>
      {:else if at.kind === "more"}
        {@const file = at.file}
        <div class="row more" style:top={`${String(place * ROW)}px`}>
          <span class="expand"
            ><button
              type="button"
              class="step"
              aria-label={`Show ${String(EXPAND)} more lines`}
              title={`Show ${String(EXPAND)} more lines`}
              onclick={() => {
                void diff.expandRest(file);
              }}>↓</button
            ></span
          >{@render whole(file)}
        </div>
      {:else}
        {@const index = at.row}
        {@const row = diff.row(index)}
        {#if sideBySide && row?.kind === "context"}
          <div class="row split" style:top={`${String(place * ROW)}px`}>
            {@render both("context", row.old, row.new, row.text, diff.spans(index) ?? [])}
          </div>
        {:else}
          <div class={`row ${row?.kind ?? "waiting"}`} style:top={`${String(place * ROW)}px`}>
            {#if row?.kind === "file"}
              {@const file = generated(index)}
              {#if file === undefined}
                <span class="path">{named(row)}</span><span class="count">{counted(row.change)}</span>
              {:else}
                <button
                  type="button"
                  class="toggle"
                  aria-expanded={!diff.collapsed(file)}
                  onclick={() => {
                    diff.toggle(file);
                  }}
                  ><span class="chevron">{diff.collapsed(file) ? "▸" : "▾"}</span><span class="path"
                    >{named(row)}</span
                  ></button
                ><span class="count">{counted(row.change)}</span><span class="count">Generated</span>
              {/if}
            {:else if row?.kind === "hunk"}
              {@const hidden = diff.layout.gapAt(index)}
              {#if hidden === undefined}
                <span class="number"></span><span class="number"></span>
              {:else}
                {@render gap(hidden)}
              {/if}<span class="text">{row.text}</span>
            {:else if row?.kind === "note"}
              <span class="number"></span><span class="number"></span><span class="text">{row.text}</span>
            {:else if row?.kind === "context"}
              <span class="number">{row.old}</span><span class="number">{row.new}</span>{@render code(
                " ",
                row.text,
                diff.spans(index) ?? [],
              )}
            {:else if row?.kind === "added"}
              <span class="number"></span><span class="number">{row.new}</span>{@render code(
                "+",
                row.text,
                diff.spans(index) ?? [],
              )}
            {:else if row?.kind === "removed"}
              <span class="number">{row.old}</span><span class="number"></span>{@render code(
                "-",
                row.text,
                diff.spans(index) ?? [],
              )}
            {/if}
          </div>
        {/if}
      {/if}
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

  /* A Generated file's header opens and closes it, and looks like any other header. */
  .toggle {
    display: flex;
    gap: 4px;
    padding: 0;
    border: 0;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
  }

  .chevron {
    width: 1ch;
  }

  .hunk,
  .more {
    background: color-mix(in srgb, #0969da 10%, transparent);
    opacity: 0.8;
  }

  /* A gap's buttons take the place of the two columns of line numbers. */
  .expand {
    display: flex;
    flex: none;
    width: 96px;
  }

  .step,
  .whole {
    padding: 0 6px;
    border: 0;
    font: inherit;
    color: inherit;
    background: none;
    cursor: pointer;
  }

  .step {
    width: 24px;
    padding: 0;
  }

  .whole {
    flex: none;
    margin-right: 8px;
    text-decoration: underline;
  }

  .step:hover,
  .whole:hover {
    background: color-mix(in srgb, #0969da 20%, transparent);
  }

  /* Side by side, each side takes half the row, and keeps its own background. */
  .side {
    display: flex;
    flex: 1 1 50%;
    min-width: 0;
  }

  /* Side by side, a row stays as wide as the tab, and scrolling across moves the code on both sides while
     the line numbers stay. Each side holds as much of its line as fits, so the halves stay in view however
     long the PR's longest line is. */
  .split {
    left: var(--across);
    right: auto;
    width: var(--width);
  }

  .clip {
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  /* The code moves across inside its side's clip, the only clip a side has. */
  .clip > .text {
    margin-left: calc(-1 * var(--across));
  }

  .side + .side {
    border-left: 1px solid color-mix(in srgb, currentColor 15%, transparent);
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

  .text :global(.hl-tag) {
    color: var(--hl-tag);
  }

  .text :global(.hl-heading) {
    color: var(--hl-constant);
    font-weight: 600;
  }

  .text :global(.hl-strong) {
    font-weight: 600;
  }

  .text :global(.hl-emphasis) {
    font-style: italic;
  }

  /* github.com's light palette. The window has no dark theme yet: one that sets `data-theme="dark"` on the
     root gets github.com's dark palette below. */
  .scroller {
    --hl-keyword: #cf222e;
    --hl-string: #0a3069;
    --hl-comment: #59636e;
    --hl-constant: #0550ae;
    --hl-entity: #6639ba;
    --hl-tag: #116329;
  }

  :global(:root[data-theme="dark"]) .scroller {
    --hl-keyword: #ff7b72;
    --hl-string: #a5d6ff;
    --hl-comment: #9198a1;
    --hl-constant: #79c0ff;
    --hl-entity: #d2a8ff;
    --hl-tag: #7ee787;
  }
</style>
