<!-- The hit's code in the Definition peek (#439): its file with line numbers in the gutter, scrolled so the
     hit's line sits a few rows from the top, and marked. It scrolls inside the peek, and draws only the rows
     near what shows, asking the core for their lines as they come into view, highlighted as the diff's rows
     are (#455). In the peek a click puts the caret on a word and a double-click selects it, as in the diff,
     for `Alt+F12` to look it up (#447). The file view shows its file the same way (#453), and `data-scrolls`
     lets the Detail pane's ↑ and ↓ scroll it there. The file view picks words too, goes to a word's
     definition on Ctrl+click, Cmd+click on macOS, and opens the Definition peek under one of its lines as an
     inset, which pushes the lines after it down (#457). -->
<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import type { CodePick, HitCode } from "$lib/state/hit-code.svelte";
  import type { Peek } from "$lib/state/peek";
  import { currentOs, goesToDefinition } from "./keys";
  import { wordPicker } from "./picking";
  import { pieces } from "./spans";

  let {
    code,
    line,
    landing,
    inset,
    ongo,
  }: {
    code: HitCode;
    /** The line it marks, counted from 1. */
    line: number;
    /** A new one scrolls the code to `line` again, even the line it already marks. */
    landing?: object;
    /** The Definition peek `peek` under line `line`, in `rows` rows, which `box` draws from its top in
     * pixels, in those rows. A new `reveal` scrolls the code as little as it takes to show it, and
     * `onreveal` hears it was taken. */
    inset?:
      | {
          peek: Peek;
          line: number;
          rows: number;
          reveal: boolean;
          onreveal: () => void;
          box: Snippet<[number, Peek, number]>;
        }
      | undefined;
    /** Goes to the definition of the word a Ctrl+click picked. */
    ongo?: () => void;
  } = $props();

  const ROW = 20;
  /** The rows over the hit's line. */
  const LEAD = 3;
  /** The rows drawn past each end of what shows. */
  const OVERSCAN = 20;
  const os = currentOs();
  /** Names this code's inset, so a click in the code of a peek it holds leaves this code's pick be. */
  const id = $props.id();
  let region = $state<HTMLElement>();
  let scrollTop = $state(0);
  let scrollLeft = $state(0);
  let height = $state(0);
  let width = $state(0);
  /** Until the file's length is known, the hit's line and a screen past it. */
  const count = $derived(code.count ?? line + 2 * OVERSCAN);
  /** The line the inset shows under, and the rows it takes. */
  const under = $derived(inset?.line ?? Infinity);
  const extra = $derived(inset?.rows ?? 0);
  /** The line that row `row`, counted from 0, shows, or the inset's line for a row the inset takes. */
  const lineOfRow = (row: number) => (row < under ? row + 1 : Math.max(under, row - extra + 1));
  /** The top of line `at`, in pixels: the lines after the inset start below it. */
  const topOf = (at: number) => (at - 1 + (at > under ? extra : 0)) * ROW;
  const first = $derived(Math.max(1, lineOfRow(Math.floor(scrollTop / ROW)) - OVERSCAN));
  const last = $derived(
    Math.min(count, lineOfRow(Math.ceil((scrollTop + Math.max(height, ROW)) / ROW) - 1) + OVERSCAN),
  );
  /** The numbers of the lines drawn. */
  const drawn = $derived(Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i));

  /** Scrolls `region` to a new hit, or back to where the user left this one's code when the diff draws the
   * peek again. The region stays while the code shows, so it attaches this once and reruns it on each hit,
   * and on each landing. */
  function scrollToHit(region: HTMLElement) {
    const [shown, target] = [code, line];
    // A new landing scrolls to the line again, though it is the one marked.
    void landing;
    untrack(() => {
      region.scrollTop = shown.scrollTop ?? Math.max(0, topOf(target) - LEAD * ROW);
      scrollTop = region.scrollTop;
    });
  }

  $effect(() => {
    code.show(first, last);
  });

  // Scrolls as little as it takes to show the whole inset, but never so far that its line leaves the top,
  // once on each opening, as the diff does for its peek (#458). Without a height there is nothing on screen
  // to bring it into.
  $effect(() => {
    if (!inset?.reveal) return;
    untrack(() => {
      inset.onreveal();
      const scroller = region as HTMLElement;
      if (height === 0) return;
      const wanted = Math.min((under + extra) * ROW - height, (under - 1) * ROW);
      if (wanted > scroller.scrollTop) scroller.scrollTop = wanted;
      scrollTop = scroller.scrollTop;
    });
  });

  /** Picks words in the code's lines, each of which names its number. */
  const picker = wordPicker<CodePick>(
    "[data-code-line]",
    {
      pick: (text, caret) => code.pickWord(Number(text.dataset["codeLine"]), caret, false),
      drop: () => {
        code.pick = undefined;
      },
      current: () => code.pick,
      select: (picked) => {
        code.pickWord(picked.line, picked.start, true);
      },
    },
    `[data-inset="${id}"]`,
  );
</script>

<!-- The region stays while the file can't be read and says why inside it, so a focus in it stays put and the
     keys keep going where they went, never to the page under it. A click picks a word in it, which the keys
     can't yet, as in the diff. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="code mono"
  role="region"
  aria-label="Definition's code"
  tabindex="0"
  data-scrolls
  bind:this={region}
  {@attach scrollToHit}
  bind:clientHeight={height}
  bind:clientWidth={width}
  onscroll={(e) => {
    scrollTop = e.currentTarget.scrollTop;
    scrollLeft = e.currentTarget.scrollLeft;
    code.scrollTop = scrollTop;
  }}
  onclick={(e) => {
    const picked = picker.click(e);
    if (picked !== undefined && goesToDefinition(e, os)) ongo?.();
  }}
  ondblclick={picker.doubleClick}
>
  {#if code.problem !== undefined}
    <p class="problem sans" role="alert">{code.problem}</p>
  {:else}
    <!-- The rows are placed absolutely, so the spacer is as wide as the widest line read and the gutter. -->
    <div
      class="spacer"
      style:height={`${String((count + extra) * ROW)}px`}
      style:min-width={`calc(${String(code.widest)}ch + 56px)`}
      style:--across={`${String(scrollLeft)}px`}
      style:--width={`${String(width)}px`}
    >
      {#each drawn as at (at)}
        {@const text = code.line(at)}
        <!-- prettier-ignore -->
        <div class={["line", at === line && "hit"]} style:top={`${String(topOf(at))}px`}><span class="number">{at}</span><span class="text" data-code-line={at}>{#each pieces(text ?? "", code.spans(at) ?? [], [], code.pick?.line === at ? code.pick : undefined) as piece, index (index)}{#if piece.caret}<span class="caret"></span>{/if}{#if piece.highlight || piece.picked}<span class={[piece.highlight && `hl-${piece.highlight}`, piece.picked && "picked"]}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span></div>
      {/each}
      {#if inset !== undefined}
        <div class="inset" data-inset={id}>{@render inset.box(under * ROW, inset.peek, extra)}</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .problem {
    margin: 0;
    padding: 0 8px;
    line-height: 20px;
  }

  .code {
    height: 100%;
    overflow: auto;
  }

  .code:focus-visible {
    outline: 1px solid var(--focus-border);
    outline-offset: -1px;
  }

  /* Its own stacking context, so an inset's edge rises only above the lines. */
  .spacer {
    position: relative;
    isolation: isolate;
  }

  /* Lays the inset's parts out in the spacer as if it weren't there. */
  .inset {
    display: contents;
  }

  .line {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    height: 20px;
    line-height: 20px;
    white-space: pre;
    tab-size: 8;
  }

  .hit {
    background: var(--find);
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

  /* The word the user picked, as the diff draws it, with a caret that takes no room. */
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
</style>
