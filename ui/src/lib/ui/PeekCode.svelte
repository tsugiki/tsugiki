<!-- The hit's code in the Definition peek (#439): its file with line numbers in the gutter, scrolled so the
     hit's line sits a few rows from the top, and marked. It scrolls inside the peek, and draws only the rows
     near what shows, asking the core for their lines as they come into view, highlighted as the diff's rows
     are (#455). In the peek a click puts the caret on a word and a double-click selects it, as in the diff,
     for `Alt+F12` to look it up (#447). The file view shows its file the same way (#453), and `data-scrolls`
     lets the Detail pane's ↑ and ↓ scroll it there. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { CodePick, HitCode } from "$lib/state/hit-code.svelte";
  import { wordPicker } from "./picking";
  import { pieces } from "./spans";

  let { code, line, pickable = false }: { code: HitCode; line: number; pickable?: boolean } = $props();

  const ROW = 20;
  /** The rows over the hit's line. */
  const LEAD = 3;
  /** The rows drawn past each end of what shows. */
  const OVERSCAN = 20;
  let scrollTop = $state(0);
  let height = $state(0);
  /** Until the file's length is known, the hit's line and a screen past it. */
  const count = $derived(code.count ?? line + 2 * OVERSCAN);
  const first = $derived(Math.max(1, Math.floor(scrollTop / ROW) + 1 - OVERSCAN));
  const last = $derived(Math.min(count, Math.ceil((scrollTop + Math.max(height, ROW)) / ROW) + OVERSCAN));
  /** The numbers of the lines drawn. */
  const drawn = $derived(Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i));
  /** The word picked in the code, which only the peek shows. */
  const pick = $derived(pickable ? code.pick : undefined);

  /** Scrolls `region` to a new hit, or back to where the user left this one's code when the diff draws the
   * peek again. The region stays while the code shows, so it attaches this once and reruns it on each hit. */
  function scrollToHit(region: HTMLElement) {
    const [shown, target] = [code, line];
    untrack(() => {
      region.scrollTop = shown.scrollTop ?? Math.max(0, (target - 1 - LEAD) * ROW);
      scrollTop = region.scrollTop;
    });
  }

  $effect(() => {
    code.show(first, last);
  });

  /** Picks words in the code's lines, each of which names its number. */
  const picker = wordPicker<CodePick>("[data-code-line]", {
    pick: (text, caret) => code.pickWord(Number(text.dataset["codeLine"]), caret, false),
    drop: () => {
      code.pick = undefined;
    },
    current: () => code.pick,
    select: (picked) => {
      code.pickWord(picked.line, picked.start, true);
    },
  });
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
  {@attach scrollToHit}
  bind:clientHeight={height}
  onscroll={(e) => {
    scrollTop = e.currentTarget.scrollTop;
    code.scrollTop = scrollTop;
  }}
  onclick={pickable ? picker.click : undefined}
  ondblclick={pickable ? picker.doubleClick : undefined}
>
  {#if code.problem !== undefined}
    <p class="problem sans" role="alert">{code.problem}</p>
  {:else}
    <!-- The rows are placed absolutely, so the spacer is as wide as the widest line read and the gutter. -->
    <div
      class="spacer"
      style:height={`${String(count * ROW)}px`}
      style:min-width={`calc(${String(code.widest)}ch + 56px)`}
    >
      {#each drawn as at (at)}
        {@const text = code.line(at)}
        <!-- prettier-ignore -->
        <div class={["line", at === line && "hit"]} style:top={`${String((at - 1) * ROW)}px`}><span class="number">{at}</span><span class="text" data-code-line={at}>{#each pieces(text ?? "", code.spans(at) ?? [], [], pick?.line === at ? pick : undefined) as piece, index (index)}{#if piece.caret}<span class="caret"></span>{/if}{#if piece.highlight || piece.picked}<span class={[piece.highlight && `hl-${piece.highlight}`, piece.picked && "picked"]}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span></div>
      {/each}
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

  .spacer {
    position: relative;
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
