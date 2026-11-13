<!-- The rows of a PR's diff, drawn only near what the tab shows, below its NEWS entries. Every row has the
     same height, so a row's place follows from where it shows, and the core sends the rows in chunks as they
     come near (ADR 0001). A collapsed file shows only its header (#181, #581), which says whether the file
     changed since the user's Stale review, or only in whitespace (#387). A hunk's row offers to
     show the lines its gap leaves out, and the row after a file's last hunk the lines after it (#182).
     Side by side, a line of context shows on both sides, and a change shows its removed rows on the left
     beside its added ones on the right (#183). A review thread takes as many rows' places under its line
     as it is tall, and the tab measures it once drawn (#184). A thread on a line no hunk shows opens the
     lines around it, with buttons above and below for the lines still hidden, and the threads on files no
     longer in the diff come under a heading after the last file (#185). One file at a time, it shows the
     page on show: the NEWS entries, one file, or those threads (#605). A click on a word of code puts the
     caret on it and highlights it, and a double-click selects it, for Go to definition (#437). -->
<script lang="ts">
  import { tick, untrack } from "svelte";
  import type { Column, Pick, PickedLine } from "$lib/state/pick";
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
  /** The NEWS entries, which show above all the files, or on a page of their own one file at a time. */
  const newsShown = $derived(diff.shown === undefined || diff.shown.kind === "news" ? news : []);

  $effect(() => {
    diff.need(first, last);
  });

  // The tab goes to the file after the one at the top, and shows that one alone as it switches.
  $effect(() => {
    diff.scrolled(Math.floor((scrollTop - newsHeight) / ROW));
  });

  /** The layout the rows were last drawn from, which a new layout starts from. */
  let drawn: PrFilesState["layout"] | undefined;
  /** Whether the NEWS entries showed above the rows last drawn. */
  let drawnNews = false;
  /** Scrolls to the thread, the change or the match the user moved to, with a few rows of code above it.
   * Otherwise, keeps the place at the top in view as the layout changes, as the view switches or a thread
   * above it grows: the same row, line, button or thread, as far into it as it was. */
  $effect(() => {
    const layout = diff.layout;
    const jump = diff.jump;
    const before = drawn;
    const newsBefore = drawnNews;
    drawn = layout;
    drawnNews = untrack(() => newsShown.length > 0);
    untrack(() => {
      const target = scrolledTo(layout, before, jump, newsBefore !== drawnNews);
      if (target === undefined || scroller === undefined || newsBox === undefined) return;
      scroller.scrollTop = target === "top" ? 0 : newsBox.offsetHeight + target.place * ROW + target.into;
      // The rows to draw follow at once, as the scroll event comes after the next frame.
      scrollTop = scroller.scrollTop;
    });
  });

  /** The place the tab scrolls to as `layout` replaces `before` or the user moves to a thread, a change, a
   * match or a page, and how many pixels into it, if it scrolls, or the top for the NEWS entries, which
   * sit above every place. It takes the move, so a later layout doesn't scroll back to it. `newsMoved`
   * says the NEWS entries above the rows went or came back with the layout, as switching between all
   * the files and one at a time does, so the same place sits higher or lower. */
  function scrolledTo(
    layout: PrFilesState["layout"],
    before: PrFilesState["layout"] | undefined,
    jump: PrFilesState["jump"],
    newsMoved: boolean,
  ): { place: number; into: number } | "top" | undefined {
    if (jump !== undefined) {
      diff.jump = undefined;
      if ("page" in jump) {
        if (jump.page.kind === "news") return "top";
        const header = layout.headerOf(jump.page);
        return header === undefined ? undefined : { place: header, into: 0 };
      }
      const place = "id" in jump ? layout.threadAt(jump.id) : layout.placeOfRow(jump.row);
      if (place === undefined || ("row" in jump && inView(place))) return undefined;
      return { place: Math.max(0, place - LEAD), into: 0 };
    }
    // `newsHeight` is still the height of the NEWS entries above the rows last drawn.
    const into = scrollTop - newsHeight;
    if (before === undefined || into < 0) return undefined;
    const place = Math.floor(into / ROW);
    const moved = layout.moved(before, place);
    return moved === place && !newsMoved ? undefined : { place: moved, into: into % ROW };
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

  /** Picks the word at offset `offset` into `node` for a click on `target`, and returns the pick. It drops
   * the pick when no word of a line of code is there, or the click landed outside that line's code. A
   * line's code starts with its marker: a caret before it is on no word, and one after it is at the start
   * of the line, as in an editor. */
  const pickAt = (target: EventTarget | null, node: Node | null, offset: number): Pick | undefined => {
    const element =
      (node instanceof Element ? node : node?.parentElement)?.closest<HTMLElement>(
        ".text:is([data-row], [data-line])",
      ) ?? undefined;
    // A click past the end of a line, or on a line's number, can leave the caret in some line's code.
    if (node === null || element === undefined || !(target instanceof Node) || !element.contains(target)) {
      diff.dropPick();
      return undefined;
    }
    const data = element.dataset;
    const at: PickedLine =
      data["row"] === undefined
        ? { kind: "line", file: Number(data["file"]), line: Number(data["line"]) }
        : { kind: "row", row: Number(data["row"]) };
    const column = data["column"] === "left" || data["column"] === "right" ? data["column"] : null;
    const before = document.createRange();
    before.setStart(element, 0);
    before.setEnd(node, offset);
    return diff.pickWord(at, column, before.toString().length - 1, false);
  };

  /** The pick the last single click made, which a double-click selects. */
  let clicked: Pick | undefined;

  /** Selects the word the user picked, once it is drawn picked. */
  const selectPicked = (region: HTMLElement) => {
    const picked = [...region.querySelectorAll(".picked")];
    // From before the word's first piece to after its last: the end moves forward over the pieces, and
    // then the start moves back over them.
    const range = document.createRange();
    for (const piece of picked) range.setEndAfter(piece);
    for (const piece of picked.reverse()) range.setStartBefore(piece);
    getSelection()?.removeAllRanges();
    getSelection()?.addRange(range);
  };

  const counted = (change: FileChange) =>
    change.kind === "binary" ? "binary" : `+${String(change.added)} −${String(change.removed)}`;
  const named = (file: { path: string; old_path: string | null }) =>
    file.old_path === null ? file.path : `${file.old_path} → ${file.path}`;
  /** The place among the files of the file whose header is row `index` of the diff, if the tab has it. */
  const fileOfHeader = (index: number) => {
    const at = diff.layout.fileAt(index);
    return at !== undefined && files[at] !== undefined ? at : undefined;
  };
</script>

<!-- A line of code after its marker, highlighted once its spans come, with the search's matches in it
     marked. Each piece sits on one line, as the row keeps its text's spaces. -->
<!-- prettier-ignore -->
{#snippet code(marker: string, text: string, spans: Span[], found: Match[], where: PickedLine, column: Column)}<span class="text" data-row={where.kind === "row" ? where.row : undefined} data-file={where.kind === "line" ? where.file : undefined} data-line={where.kind === "line" ? where.line : undefined} data-column={column ?? undefined}>{marker}{#each pieces(text, spans, found, diff.pickedIn(where, column)) as piece, at (at)}{#if piece.caret}<span class="caret"></span>{/if}{#if piece.highlight || piece.found || piece.picked}<span class={[piece.highlight && `hl-${piece.highlight}`, piece.found, piece.picked && "picked"]}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span>{/snippet}

<!-- One side of a row side by side: a line's number and its code. -->
<!-- prettier-ignore -->
{#snippet side(kind: string, number: number, marker: string, text: string, spans: Span[], found: Match[], where: PickedLine, column: Column)}<span class={`side ${kind}`}><span class="number">{number}</span><span class="clip">{@render code(marker, text, spans, found, where, column)}</span></span>{/snippet}

<!-- A side with nothing on it, as the right side of a removed row has, or one whose row hasn't come. -->
{#snippet blank(kind: string)}<span class={`side ${kind}`}></span>{/snippet}

<!-- A line of context side by side: its old number and its new, with its code and its matches on both
     sides. -->
{#snippet both(
  kind: string,
  old: number,
  line: number,
  text: string,
  spans: Span[],
  found: Match[],
  where: PickedLine,
)}
  {@render side(kind, old, " ", text, spans, found, where, "left")}{@render side(
    kind,
    line,
    " ",
    text,
    spans,
    found,
    where,
    "right",
  )}
{/snippet}

<!-- Side by side, one side of a change: row `index` of the diff, removed or added, the note that the line
     above has no newline, or nothing. -->
{#snippet paired(index: number | null)}
  {@const row = index === null ? undefined : diff.row(index)}
  {#if index === null}
    {@render blank("")}
  {:else if row?.kind === "removed"}
    {@render side(
      "removed",
      row.old,
      "-",
      row.text,
      diff.spans(index) ?? [],
      diff.foundIn(index),
      {
        kind: "row",
        row: index,
      },
      "left",
    )}
  {:else if row?.kind === "added"}
    {@render side(
      "added",
      row.new,
      "+",
      row.text,
      diff.spans(index) ?? [],
      diff.foundIn(index),
      {
        kind: "row",
        row: index,
      },
      "right",
    )}
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
     pane's ↑ and ↓ scroll it too while the Issue list is hidden, which `data-scrolls` marks it for. A click
     picks a word in it, which the keys can't yet: a caret the keys move waits for after v1 (scope.md). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="scroller mono"
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
  onclick={(event) => {
    // A double-click's second click leaves the word to the double-click.
    if (event.detail > 1) return;
    clicked = undefined;
    // A button, a link or a field keeps the pick.
    const target = event.target;
    if (target instanceof Element && target.closest("button, a, input, textarea, select, summary")) return;
    const selection = getSelection();
    // A drag selects text to copy, and leaves the pick where it was.
    if (selection === null || !selection.isCollapsed) return;
    clicked = pickAt(target, selection.focusNode, selection.focusOffset);
  }}
  ondblclick={async (event) => {
    const region = event.currentTarget;
    // The browser's own word can run past an identifier, as through the dot of `self.name`, so the
    // double-click selects the word its first click picked, if that click picked one.
    const pick = clicked;
    clicked = undefined;
    if (pick === undefined || diff.picked !== pick) return;
    diff.pickWord(pick.at, pick.column, pick.start, true);
    await tick();
    selectPicked(region);
  }}
>
  <div bind:this={newsBox} bind:clientHeight={newsHeight}>
    <NewsEntries
      news={newsShown}
      {onlink}
      onfile={(path) => {
        diff.showPath(path);
      }}
    />
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
          {@render both("context expanded", at.old, at.new, line.text, line.spans, [], {
            kind: "line",
            file: at.file,
            line: at.new,
          })}
        </div>
      {:else if at.kind === "line"}
        {@const line = diff.line(at.file, at.new)}
        <div class="row context expanded" style:top={`${String(place * ROW)}px`}>
          <span class="number">{at.old}</span><span class="number">{at.new}</span>{@render code(
            " ",
            line.text,
            line.spans,
            [],
            { kind: "line", file: at.file, line: at.new },
            null,
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
              { kind: "row", row: index },
            )}
          </div>
        {:else}
          <div class={`row ${row?.kind ?? "waiting"}`} style:top={`${String(place * ROW)}px`}>
            {#if row?.kind === "file"}
              {@const file = fileOfHeader(index)}
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
                ><span class="count">{counted(row.change)}</span>{#if files[file]?.generated}<span
                    class="count">Generated</span
                  >{/if}{#if diff.sinceReview(file) === "changed"}<span class="since"
                    >Changed since your Review</span
                  >{:else if diff.sinceReview(file) === "whitespace_only"}<span class="since"
                    >Whitespace only</span
                  >{/if}
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
                { kind: "row", row: index },
                null,
              )}
            {:else if row?.kind === "added"}
              <span class="number"></span><span class="number">{row.new}</span>{@render code(
                "+",
                row.text,
                diff.spans(index) ?? [],
                diff.foundIn(index),
                { kind: "row", row: index },
                null,
              )}
            {:else if row?.kind === "removed"}
              <span class="number">{row.old}</span><span class="number"></span>{@render code(
                "-",
                row.text,
                diff.spans(index) ?? [],
                diff.foundIn(index),
                { kind: "row", row: index },
                null,
              )}
            {/if}
          </div>
        {/if}
      {/if}
    {/each}
    {#each diff.layout.threadsIn(first, last) as placed (placed.laid.thread.id)}
      <div
        class="thread sans"
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
    overscroll-behavior: contain;
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
    color: var(--fg-muted);
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
    color: var(--fg-muted);
  }

  /* A file changed since the user's Stale review, in the danger colour its badge has. */
  .since {
    font-weight: normal;
    color: var(--danger);
  }

  /* A file's header opens and closes it, and looks like the text around it. */
  .toggle {
    display: flex;
    gap: 4px;
    padding: 0;
    border: 0;
    background: none;
  }

  .chevron {
    width: 1ch;
  }

  .hunk,
  .more {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--fg-muted);
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
    background: none;
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
    background: color-mix(in srgb, var(--accent) 20%, transparent);
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
    color: var(--fg-muted);
  }

  .added {
    background: color-mix(in srgb, var(--add) 18%, transparent);
  }

  .removed {
    background: color-mix(in srgb, var(--del) 18%, transparent);
  }

  /* The search's matches show on the theme's find colour, and the one the tab is on stands out from them. */
  .text :global(.match) {
    background: color-mix(in srgb, var(--find) 40%, transparent);
  }

  .text :global(.current) {
    background: var(--find-current);
    color: var(--find-current-fg);
  }

  /* The word the user picked, on a tint of the accent, with a caret that takes no room. */
  .text :global(.picked) {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
  }

  .caret {
    display: inline-block;
    width: 0;
    height: 14px;
    margin-right: -1px;
    border-left: 1px solid currentColor;
    vertical-align: -3px;
  }

  /* The theme's syntax colours, in the rows and in the code of a thread. The classes come from the spans,
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
</style>
