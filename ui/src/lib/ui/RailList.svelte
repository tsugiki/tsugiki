<!-- A list of rows in one of the rail's tabs, as a tree: the Files tab's files (#604) and Search's results
     (#686). It draws only the rows on screen, holds the focus and names the row under the cursor. ↑ ↓,
     Home, End, Page Up and Page Down move the cursor, ← and → fold and unfold, and Enter or a click picks
     the row. The keys go no further, so the Issue list's cursor stays where it is. -->
<script module lang="ts">
  import type { ClassValue } from "svelte/elements";

  /** How a row shows, besides what its snippet draws. */
  export type RailRow = {
    /** How deep in the tree it sits, from 1. */
    level: number;
    /** Whether it is open, for a row that folds. */
    expanded?: boolean;
    /** Whether it is the one on screen elsewhere, as the file the Files tab shows. */
    current?: boolean;
    title?: string;
    class?: ClassValue;
  };
</script>

<script lang="ts">
  import { type Snippet, tick, untrack } from "svelte";
  import { currentOs, keyName } from "./keys";

  let {
    label,
    count,
    cursor,
    indent,
    shows,
    row,
    onmove,
    onfold,
    onunfold,
    onpick,
    reveal,
    top,
  }: {
    label: string;
    /** How many rows there are. */
    count: number;
    /** The row under the cursor. */
    cursor: number;
    /** How far in each level a row sits, in pixels. */
    indent: number;
    /** How row `index` shows. */
    shows: (index: number) => RailRow;
    /** What row `index` draws. */
    row: Snippet<[number]>;
    /** Puts the cursor on row `index`, which may be past either end. */
    onmove: (index: number) => void;
    onfold: () => void;
    onunfold: () => void;
    /** Picks the row under the cursor, as Enter or a click does. */
    onpick: () => void;
    /** A row to keep in view as it changes. */
    reveal?: number | undefined;
    /** Something new, such as another query's results, that scrolls the list back to its top. */
    top?: unknown;
  } = $props();

  /** A row's height in pixels. */
  const ROW = 22;
  /** The rows drawn past each end of what shows. */
  const OVERSCAN = 10;
  const os = currentOs();
  const id = $props.id();

  let list = $state<HTMLElement | null>(null);
  let scrollTop = $state(0);
  let height = $state(0);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(count, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  /** The rows drawn: those on screen, and a few past each end. */
  const drawn = $derived(Array.from({ length: Math.max(0, last - first) }, (_, at) => first + at));

  /** Gives the list the focus, as ↓ in the box above it does. */
  export function focus() {
    list?.focus();
  }

  // An effect runs once the list is drawn, so it is bound.
  $effect(() => {
    const [scroller] = [list, top];
    untrack(() => {
      (scroller as HTMLElement).scrollTop = 0;
      scrollTop = 0;
    });
  });

  $effect(() => {
    const [scroller, index] = [list, reveal];
    untrack(() => {
      if (index !== undefined) show(scroller as HTMLElement, index);
    });
  });

  /** Scrolls `scroller`, the list, as little as it takes to show row `index`. */
  function show(scroller: HTMLElement, index: number) {
    const at = index * ROW;
    scroller.scrollTop = Math.min(at, Math.max(scroller.scrollTop, at + ROW - scroller.clientHeight));
    scrollTop = scroller.scrollTop;
  }

  const pageRows = () => Math.max(1, Math.floor(height / ROW) - 1);

  async function onkeydown(e: KeyboardEvent & { currentTarget: HTMLElement }) {
    const moves: Record<string, () => void> = {
      arrowdown: () => {
        onmove(cursor + 1);
      },
      arrowup: () => {
        onmove(cursor - 1);
      },
      home: () => {
        onmove(0);
      },
      end: () => {
        onmove(count - 1);
      },
      pagedown: () => {
        onmove(cursor + pageRows());
      },
      pageup: () => {
        onmove(cursor - pageRows());
      },
      arrowleft: onfold,
      arrowright: onunfold,
      enter: onpick,
    };
    const move = moves[keyName(e, os) ?? ""];
    if (move === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    const scroller = e.currentTarget;
    move();
    await tick();
    show(scroller, cursor);
  }

  /** The id of row `index`, which the list names as the row under the cursor. */
  const rowId = (index: number) => `${id}-row-${String(index)}`;
</script>

<div
  class="list"
  role="tree"
  aria-label={label}
  tabindex="0"
  aria-activedescendant={cursor >= first && cursor < last ? rowId(cursor) : undefined}
  bind:this={list}
  bind:clientHeight={height}
  onscroll={(e) => {
    scrollTop = e.currentTarget.scrollTop;
  }}
  onkeydown={(e) => void onkeydown(e)}
>
  <div class="spacer" style:height={`${String(count * ROW)}px`}>
    {#each drawn as index (index)}
      {@const shown = shows(index)}
      <!-- The list holds the focus, and names the row under the cursor, so a row takes none. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
      <div
        id={rowId(index)}
        class={["row", shown.class, index === cursor && "cursor", shown.current && "current"]}
        role="treeitem"
        aria-level={shown.level}
        aria-selected={index === cursor}
        aria-expanded={shown.expanded}
        aria-current={shown.current ? "true" : undefined}
        title={shown.title}
        style:top={`${String(index * ROW)}px`}
        style:padding-left={`${String(8 + (shown.level - 1) * indent)}px`}
        onclick={() => {
          onmove(index);
          onpick();
        }}
      >
        {@render row(index)}
      </div>
    {/each}
  </div>
</div>

<style>
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .list:focus-visible {
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  .spacer {
    position: relative;
  }

  .row {
    position: absolute;
    right: 0;
    left: 0;
    display: flex;
    gap: 6px;
    align-items: center;
    box-sizing: border-box;
    height: 22px;
    padding-right: 8px;
    white-space: nowrap;
    cursor: pointer;
  }

  .row:hover {
    background: color-mix(in srgb, currentColor 6%, transparent);
  }

  /* The row under the cursor, and the one on screen elsewhere, in the selection's dim tint, and the cursor
     in the active selection colour while the list holds the focus, as the Issue list draws its rows
     (#602). */
  .row.cursor,
  .row.current {
    background: var(--selection-inactive);
  }

  .list:focus .row.cursor {
    background: var(--selection-active);
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  /* A Generated file's rows greyed, as the Files tab collapses it. */
  .row:global(.generated) {
    color: var(--fg-muted);
  }

  /* Windows' contrast themes drop the tints, so the row on screen elsewhere keeps a border. */
  @media (forced-colors: active) {
    .row.current {
      outline: 1px dashed Highlight;
      outline-offset: -1px;
    }
  }
</style>
