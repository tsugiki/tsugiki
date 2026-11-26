<!-- The hit's code in the Definition peek (#439): its file with line numbers in the gutter, scrolled so the
     hit's line sits a few rows from the top, and marked. It takes the diff's size and marks the line as the
     diff does, from window.css (#754). It scrolls inside the peek, and draws only the rows near what shows,
     asking the core for their lines as they come into view, highlighted as the diff's rows are (#455). In
     the peek a click puts the caret on a word and a double-click selects it, as in the diff, for `Alt+F12`
     to look it up (#447). The file view shows its file the same way (#453), and `data-scrolls` marks it as
     a region the Detail pane's scroll keys move there. The file view picks words too, goes to a word's
     definition on Ctrl+click, Cmd+click on macOS, and opens the Definition peek under one of its lines as
     an inset, which pushes the lines after it down (#457), or split below it, which keeps the line it opened
     under in view (#459). -->
<script lang="ts">
  import { untrack, type Snippet } from "svelte";
  import type { CodePick, HitCode } from "$lib/state/hit-code.svelte";
  import { ROW, type Peek } from "$lib/state/peek";
  import { currentOs, goesToDefinition } from "./keys";
  import { wordPicker } from "./picking";
  import CodePieces from "./CodePieces.svelte";

  let {
    code,
    mark,
    inset,
    sight,
    ongo,
  }: {
    code: HitCode;
    /** The line it marks and scrolls to, counted from 1: the hit's, or the file view's landing. A new one
     * scrolls to its line again, even the line marked before. */
    mark: { line: number };
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
    /** The line a split Definition peek below the code opened under (#459), which takes the code's room. A
     * new `reveal` scrolls the code as little as it takes to keep that line in view, and `onreveal` hears it
     * was taken. */
    sight?: { line: number; reveal: boolean; onreveal: () => void } | undefined;
    /** Goes to the definition of the word a Ctrl+click picked. */
    ongo?: () => void;
  } = $props();

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
  const count = $derived(code.count ?? mark.line + 2 * OVERSCAN);
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

  /** Scrolls `region` to a new hit or landing, or back to where the user left this one's code when the diff
   * draws the peek again. The region stays while the code shows, so it attaches this once and reruns it on
   * each mark. */
  function scrollToHit(region: HTMLElement) {
    const [shown, target] = [code, mark.line];
    untrack(() => {
      region.scrollTop = shown.scrollTop ?? Math.max(0, topOf(target) - LEAD * ROW);
      scrollTop = region.scrollTop;
    });
  }

  $effect(() => {
    code.show(first, last);
  });

  /** The inset's line and rows as last laid out. */
  let laid = { under: Infinity, extra: 0 };
  /** The pixels of the inset laid out as `at` that lie above `y`. */
  const insetAbove = (at: typeof laid, y: number) =>
    Math.min(Math.max(y - at.under * ROW, 0), at.extra * ROW);

  // Holds the line at the top where it is as the inset opens, closes or changes its rows above it, as the
  // diff holds its place when a thread above it grows. A top inside the inset stays there as the inset
  // changes its rows, as far as its new end, and goes to the line after it as it closes.
  $effect(() => {
    const now = { under, extra };
    untrack(() => {
      const scroller = region as HTMLElement;
      const [was, at] = [laid, scroller.scrollTop];
      laid = now;
      const inside = at > was.under * ROW && at < (was.under + was.extra) * ROW;
      const content = at - insetAbove(was, at);
      const top =
        inside && now.under === was.under
          ? Math.min(at, (now.under + now.extra) * ROW)
          : content < now.under * ROW
            ? content
            : content + now.extra * ROW;
      if (top === at) return;
      scroller.scrollTop = top;
      scrollTop = top;
    });
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

  // Keeps the line a split peek opened under in view, once on each opening, as the code measures now that
  // the peek below it shows (#459).
  $effect(() => {
    if (!sight?.reveal) return;
    untrack(() => {
      sight.onreveal();
      const scroller = region as HTMLElement;
      const wanted = topOf(sight.line) + ROW - scroller.clientHeight;
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
  class="code mono code-lines"
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
    <!-- The rows are placed absolutely, so the spacer is as wide as the widest line read and the gutter. An
         inset stays as wide as the code, however far across it scrolls, from `--across` and `--width`, which
         code without one leaves unset. -->

    <div
      class="spacer"
      style:height={`${String((count + extra) * ROW)}px`}
      style:min-width={`calc(${String(code.widest)}ch + 56px)`}
      style:--across={inset && `${String(scrollLeft)}px`}
      style:--width={inset && `${String(width)}px`}
    >
      {#each drawn as at (at)}
        {@const text = code.line(at)}
        <!-- prettier-ignore -->
        <div class={["row", "line", at === mark.line && "landed"]} style:top={`${String(topOf(at))}px`}><span class="number">{at}</span><span class="text" data-code-line={at}><CodePieces text={text ?? ""} spans={code.spans(at) ?? []} picked={code.pick?.line === at ? code.pick : undefined} /></span></div>
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
  }
</style>
