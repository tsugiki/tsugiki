<!-- The rows of a PR's diff, drawn only near what the tab shows, below its NEWS entries. Every row has the
     same height, so a row's place follows from where it shows, and the core sends the rows in chunks as they
     come near (ADR 0001). A collapsed Generated file shows only its header (#181). A hunk's row offers to
     show the lines its gap leaves out, and the row after a file's last hunk the lines after it (#182).
     Side by side, a line of context shows on both sides, and a change shows its removed rows on the left
     beside its added ones on the right (#183). A review thread takes as many rows' places under its line
     as it is tall, and the tab measures it once drawn (#184). A thread on a line no hunk shows opens the
     lines around it, with buttons above and below for the lines still hidden, and the threads on files no
     longer in the diff come under a heading after the last file (#185). -->
<script lang="ts">
  import { untrack } from "svelte";
  import {
    EXPAND,
    type DiffFile,
    type FileChange,
    type HiddenLines,
    type NewsEntry,
    type PrFilesState,
    type Span,
  } from "$lib/state/pr-files.svelte";
  import NewsEntries from "./NewsEntries.svelte";
  import ReviewThread from "./ReviewThread.svelte";
  import { pieces, type Match } from "./spans";

  let {
    diff,
    files,
    news,
    now,
    onlink,
  }: {
    diff: PrFilesState;
    files: DiffFile[];
    news: NewsEntry[];
    now: Date;
    onlink: (address: string) => void;
  } = $props();

  /** A row's height in pixels. */
  const ROW = 20;
  /** Rows drawn above and below the visible ones, so a quick scroll doesn't show blanks. */
  const OVERSCAN = 40;
  /** Rows of code that show above a thread, a change or a match the tab moves to. */
  const LEAD = 3;

  let scroller = $state<HTMLElement>();
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let height = $state(0);
  let width = $state(0);
  /** How tall the NEWS entries above the rows are. */
  let newsHeight = $state(0);
  /** The NEWS entries, whose height a scroll right after the tab opens reads before `newsHeight` has it. */
  let newsBox = $state<HTMLElement>();
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

  /** The layout the rows were last drawn from, which a new layout starts from. */
  let drawn: PrFilesState["layout"] | undefined;
  /** Scrolls to the thread, the change or the match the user moved to, with a few rows of code above it.
   * Otherwise, keeps the place at the top in view as the layout changes, as the view switches or a thread
   * above it grows: the same row, line, button or thread, as far into it as it was. */
  $effect(() => {
    const layout = diff.layout;
    const jump = diff.jump;
    const before = drawn;
    drawn = layout;
    untrack(() => {
      const target = scrolledTo(layout, before, jump);
      if (target === undefined || scroller === undefined || newsBox === undefined) return;
      scroller.scrollTop = newsBox.offsetHeight + target.place * ROW + target.into;
      // The rows to draw follow at once, as the scroll event comes after the next frame.
      scrollTop = scroller.scrollTop;
    });
  });

  /** The place the tab scrolls to as `layout` replaces `before` or the user moves to a thread, a change or a
   * match, and how many pixels into it, if it scrolls. It takes the move, so a later layout doesn't scroll
   * back to it. */
  function scrolledTo(
    layout: PrFilesState["layout"],
    before: PrFilesState["layout"] | undefined,
    jump: PrFilesState["jump"],
  ): { place: number; into: number } | undefined {
    if (jump !== undefined) {
      diff.jump = undefined;
      const place = "id" in jump ? layout.threadAt(jump.id) : layout.placeOfRow(jump.row);
      if (place === undefined || ("row" in jump && inView(place))) return undefined;
      return { place: Math.max(0, place - LEAD), into: 0 };
    }
    const into = scrollTop - newsHeight;
    if (before === undefined || into <= 0) return undefined;
    const place = Math.floor(into / ROW);
    const moved = layout.moved(before, place);
    return moved === place ? undefined : { place: moved, into: into % ROW };
  }

  /** Whether place `place` shows whole: a change or a match there stays where it is, as VS Code leaves a
   * match it shows. */
  const inView = (place: number) => {
    const top = scrollTop - newsHeight;
    return place * ROW >= top && (place + 1) * ROW <= top + height;
  };

  /** Tells the tab how many places open thread `thread`, by its place among the threads, takes, as it
   * draws and whenever its height changes. */
  const measure = (id: string, thread: number) => (element: HTMLElement) => {
    const observer = new ResizeObserver(() => {
      if (!diff.folded(thread)) diff.measured(id, Math.max(1, Math.ceil(element.offsetHeight / ROW)));
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  };

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

<!-- A line of code after its marker, highlighted once its spans come, with the search's matches in it
     marked. Each piece sits on one line, as the row keeps its text's spaces. -->
<!-- prettier-ignore -->
{#snippet code(marker: string, text: string, spans: Span[], found: Match[])}<span class="text">{marker}{#each pieces(text, spans, found) as piece, at (at)}{#if piece.highlight || piece.found}<span class={[piece.highlight && `hl-${piece.highlight}`, piece.found]}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span>{/snippet}

<!-- One side of a row side by side: a line's number and its code. -->
<!-- prettier-ignore -->
{#snippet side(kind: string, number: number, marker: string, text: string, spans: Span[], found: Match[])}<span class={`side ${kind}`}><span class="number">{number}</span><span class="clip">{@render code(marker, text, spans, found)}</span></span>{/snippet}

<!-- A side with nothing on it, as the right side of a removed row has, or one whose row hasn't come. -->
{#snippet blank(kind: string)}<span class={`side ${kind}`}></span>{/snippet}

<!-- A line of context side by side: its old number and its new, with its code and its matches on both
     sides. -->
{#snippet both(kind: string, old: number, line: number, text: string, spans: Span[], found: Match[])}
  {@render side(kind, old, " ", text, spans, found)}{@render side(kind, line, " ", text, spans, found)}
{/snippet}

<!-- Side by side, one side of a change: row `index` of the diff, removed or added, the note that the line
     above has no newline, or nothing. -->
{#snippet paired(index: number | null)}
  {@const row = index === null ? undefined : diff.row(index)}
  {#if index === null}
    {@render blank("")}
  {:else if row?.kind === "removed"}
    {@render side("removed", row.old, "-", row.text, diff.spans(index) ?? [], diff.foundIn(index))}
  {:else if row?.kind === "added"}
    {@render side("added", row.new, "+", row.text, diff.spans(index) ?? [], diff.foundIn(index))}
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

<!-- The buttons for lines a gap or the end of a file still hides, in the place of the line numbers: 20 more
     from the bottom up or from the top down, or all of a short run. As on GitHub, lines from a file's start
     have nothing above them, so they show only from the bottom up. -->
{#snippet gap(hidden: HiddenLines)}
  <span class="expand">
    {#if hidden.lines > EXPAND}
      <button
        type="button"
        class="step"
        aria-label={`Show ${String(EXPAND)} lines above`}
        title={`Show ${String(EXPAND)} lines above`}
        onclick={() => {
          diff.expandAbove(hidden);
        }}>↑</button
      >{#if hidden.new > 1}<button
          type="button"
          class="step"
          aria-label={`Show ${String(EXPAND)} lines below`}
          title={`Show ${String(EXPAND)} lines below`}
          onclick={() => {
            diff.expandBelow(hidden);
          }}>↓</button
        >{/if}
    {:else}
      <button
        type="button"
        class="step"
        aria-label={`Show ${String(hidden.lines)} hidden lines`}
        title={`Show ${String(hidden.lines)} hidden lines`}
        onclick={() => {
          diff.expandGap(hidden);
        }}>↕</button
      >
    {/if}
  </span>{@render whole(hidden.file)}
{/snippet}

<!-- A region that scrolls takes focus, so the keys scroll it (axe's scrollable-region-focusable). The Detail
     pane's ↑ and ↓ scroll it too while the Issue list is hidden, which `data-scrolls` marks it for. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="scroller"
  role="region"
  aria-label={sideBySide ? "Side-by-side diff" : "Diff"}
  tabindex="0"
  data-scrolls
  bind:this={scroller}
  {@attach (region) => {
    // The search hands the diff its focus back as it closes, so the keys scroll the diff again.
    if (!diff.diffWantsFocus) return;
    diff.diffWantsFocus = false;
    region.focus();
  }}
  bind:clientHeight={height}
  bind:clientWidth={width}
  onscroll={(event) => {
    scrollTop = event.currentTarget.scrollTop;
    scrollLeft = event.currentTarget.scrollLeft;
  }}
>
  <div bind:this={newsBox} bind:clientHeight={newsHeight}>
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
      {#if at.kind === "thread"}
        <!-- Drawn below, once for all its places. -->
      {:else if at.kind === "line" && sideBySide}
        {@const line = diff.line(at.file, at.new)}
        <div class="row split" style:top={`${String(place * ROW)}px`}>
          {@render both("context expanded", at.old, at.new, line.text, line.spans, [])}
        </div>
      {:else if at.kind === "line"}
        {@const line = diff.line(at.file, at.new)}
        <div class="row context expanded" style:top={`${String(place * ROW)}px`}>
          <span class="number">{at.old}</span><span class="number">{at.new}</span>{@render code(
            " ",
            line.text,
            line.spans,
            [],
          )}
        </div>
      {:else if at.kind === "pair"}
        <div class="row split" style:top={`${String(place * ROW)}px`}>
          {@render paired(at.removed)}{@render paired(at.added)}
        </div>
      {:else if at.kind === "hidden"}
        <div class="row hunk" style:top={`${String(place * ROW)}px`}>{@render gap(at)}</div>
      {:else if at.kind === "gone"}
        <div class="row file" style:top={`${String(place * ROW)}px`}>
          <span class="path">On files no longer in the diff</span>
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
            {@render both(
              "context",
              row.old,
              row.new,
              row.text,
              diff.spans(index) ?? [],
              diff.foundIn(index),
            )}
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
                diff.foundIn(index),
              )}
            {:else if row?.kind === "added"}
              <span class="number"></span><span class="number">{row.new}</span>{@render code(
                "+",
                row.text,
                diff.spans(index) ?? [],
                diff.foundIn(index),
              )}
            {:else if row?.kind === "removed"}
              <span class="number">{row.old}</span><span class="number"></span>{@render code(
                "-",
                row.text,
                diff.spans(index) ?? [],
                diff.foundIn(index),
              )}
            {/if}
          </div>
        {/if}
      {/if}
    {/each}
    {#each diff.layout.threadsIn(first, last) as placed (placed.laid.thread.id)}
      <div
        class="thread"
        role="group"
        aria-label="Review thread"
        style:top={`${String(placed.shown * ROW)}px`}
        {@attach measure(placed.laid.thread.id, placed.thread)}
      >
        <ReviewThread
          thread={placed.laid.thread}
          folded={diff.folded(placed.thread)}
          {now}
          ontoggle={() => {
            diff.toggleThread(placed.thread);
          }}
          {onlink}
        />
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

  /* A thread stays as wide as the tab, however far across the code scrolls, and reads as prose. */
  .thread {
    position: absolute;
    left: var(--across);
    width: var(--width);
    box-sizing: border-box;
    padding-left: 96px;
    font-family: system-ui, sans-serif;
    border-top: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
    background: color-mix(in srgb, currentColor 4%, transparent);
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

  /* The search's matches show on yellow, and the one the tab is on on orange, dark text on it in either
     theme. */
  .text :global(.match) {
    background: color-mix(in srgb, #d4a72c 40%, transparent);
  }

  .text :global(.current) {
    background: #ff9632;
    color: #1f2328;
  }

  /* github.com's colors for code, in the rows and in the code of a thread. The classes come from the spans,
     which Svelte can't see, so they're global within the scroller. */
  .scroller :global(.hl-keyword) {
    color: var(--hl-keyword);
  }

  .scroller :global(.hl-string) {
    color: var(--hl-string);
  }

  .scroller :global(.hl-comment) {
    color: var(--hl-comment);
  }

  .scroller :global(.hl-constant) {
    color: var(--hl-constant);
  }

  .scroller :global(.hl-function),
  .scroller :global(.hl-type) {
    color: var(--hl-entity);
  }

  .scroller :global(.hl-tag) {
    color: var(--hl-tag);
  }

  .scroller :global(.hl-heading) {
    color: var(--hl-constant);
    font-weight: 600;
  }

  .scroller :global(.hl-strong) {
    font-weight: 600;
  }

  .scroller :global(.hl-emphasis) {
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
