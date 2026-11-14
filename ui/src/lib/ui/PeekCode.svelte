<!-- The hit's code in the Definition peek (#439): its file with line numbers in the gutter, scrolled so the
     hit's line sits a few rows from the top, and marked. It scrolls inside the peek, and draws only the rows
     near what shows, asking the core for their lines as they come into view. The lines are plain text until
     #455 highlights files outside the diff. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { HitCode } from "$lib/state/hit-code.svelte";

  let { code, line }: { code: HitCode; line: number } = $props();

  const ROW = 20;
  /** The rows over the hit's line. */
  const LEAD = 3;
  /** The rows drawn past each end of what shows. */
  const OVERSCAN = 20;
  let scroller = $state<HTMLElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  /** Until the file's length is known, the hit's line and a screen past it. */
  const count = $derived(code.count ?? line + 2 * OVERSCAN);
  const first = $derived(Math.max(1, Math.floor(scrollTop / ROW) + 1 - OVERSCAN));
  const last = $derived(Math.min(count, Math.ceil((scrollTop + Math.max(height, ROW)) / ROW) + OVERSCAN));
  /** The numbers of the lines drawn. */
  const drawn = $derived(Array.from({ length: Math.max(0, last - first + 1) }, (_, i) => first + i));

  // Scrolls to a new hit, or back to where the user left this one's code when the diff draws the peek again.
  $effect(() => {
    const [shown, target] = [code, line];
    untrack(() => {
      if (!scroller) return;
      scroller.scrollTop = shown.scrollTop ?? Math.max(0, (target - 1 - LEAD) * ROW);
      scrollTop = scroller.scrollTop;
    });
  });

  $effect(() => {
    code.show(first, last);
  });
</script>

{#if code.problem !== undefined}
  <p class="problem" role="alert">{code.problem}</p>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="code mono"
    role="region"
    aria-label="Definition's code"
    tabindex="0"
    bind:this={scroller}
    bind:clientHeight={height}
    onscroll={(e) => {
      scrollTop = e.currentTarget.scrollTop;
      code.scrollTop = scrollTop;
    }}
  >
    <!-- The rows are placed absolutely, so the spacer is as wide as the widest line read and the gutter. -->
    <div
      class="spacer"
      style:height={`${String(count * ROW)}px`}
      style:min-width={`calc(${String(code.widest)}ch + 56px)`}
    >
      {#each drawn as at (at)}
        {@const text = code.line(at)}
        <!-- prettier-ignore -->
        <div class={["line", at === line && "hit"]} style:top={`${String((at - 1) * ROW)}px`}><span class="number">{at}</span><span class="text">{text ?? ""}</span></div>
      {/each}
    </div>
  </div>
{/if}

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
</style>
