<!-- PROTOTYPE, throwaway (#365): a whole file's lines, highlighted, drawn only near what shows, scrolled so
     line `line` sits a few rows from the top and marked. Shared by the peeks and the file view. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { FileLines } from "./definition-peek.svelte";
  import { pieces } from "../spans";

  let {
    file,
    line,
    lead = 3,
    gutter = true,
  }: { file: FileLines | undefined; line: number; lead?: number; gutter?: boolean } = $props();

  const ROW = 20;
  const OVERSCAN = 30;
  let scroller = $state<HTMLElement>();
  let scrollTop = $state(0);
  let height = $state(0);
  const lines = $derived(file?.state === "done" ? file.lines : []);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - OVERSCAN));
  const last = $derived(Math.min(lines.length - 1, Math.ceil((scrollTop + height) / ROW) + OVERSCAN));
  const columns = $derived(lines.reduce((widest, one) => Math.max(widest, one.text.length), 0));

  $effect(() => {
    const target = line;
    const count = lines.length;
    untrack(() => {
      if (!scroller || count === 0) return;
      scroller.scrollTop = Math.max(0, (target - 1 - lead) * ROW);
      scrollTop = scroller.scrollTop;
    });
  });
</script>

{#if file === undefined || file.state === "loading"}
  <p class="waiting">Reading the file from the mirror…</p>
{:else if file.state === "error"}
  <p class="waiting" role="alert">{file.error}</p>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="code"
    role="region"
    aria-label="Code"
    tabindex="0"
    data-proto-code
    bind:this={scroller}
    bind:clientHeight={height}
    onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}
  >
    <div class="spacer" style:height={`${String(lines.length * ROW)}px`} style:min-width={`calc(${String(columns)}ch + 72px)`}>
      {#each { length: Math.max(0, last - first + 1) } as _, i (first + i)}
        {@const at = first + i}
        {@const one = lines[at]}
        {#if one}
          <!-- prettier-ignore -->
          <div class={["row", at + 1 === line && "target"]} data-line={at + 1} style:top={`${String(at * ROW)}px`}>{#if gutter}<span class="number">{at + 1}</span>{/if}<span class="text">{#each pieces(one.text, one.spans, []) as piece, k (k)}{#if piece.highlight}<span class={`hl-${piece.highlight}`}>{piece.text}</span>{:else}{piece.text}{/if}{/each}</span></div>
        {/if}
      {/each}
    </div>
  </div>
{/if}

<style>
  .waiting {
    margin: 8px 12px;
    font-family: system-ui, sans-serif;
    font-size: 12px;
    opacity: 0.75;
  }

  .code {
    position: absolute;
    inset: 0;
    overflow: auto;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    --hl-keyword: #cf222e;
    --hl-string: #0a3069;
    --hl-comment: #59636e;
    --hl-constant: #0550ae;
    --hl-entity: #6639ba;
    --hl-tag: #116329;
  }

  .code:focus-visible {
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

  .target {
    background: color-mix(in srgb, #d4a72c 28%, transparent);
  }

  .number {
    flex: none;
    width: 56px;
    padding-right: 10px;
    box-sizing: border-box;
    text-align: right;
    opacity: 0.55;
    user-select: none;
  }

  .code :global(.hl-keyword) {
    color: var(--hl-keyword);
  }
  .code :global(.hl-string) {
    color: var(--hl-string);
  }
  .code :global(.hl-comment) {
    color: var(--hl-comment);
  }
  .code :global(.hl-constant) {
    color: var(--hl-constant);
  }
  .code :global(.hl-function),
  .code :global(.hl-type) {
    color: var(--hl-entity);
  }
  .code :global(.hl-tag) {
    color: var(--hl-tag);
  }
  .code :global(.hl-heading) {
    color: var(--hl-constant);
    font-weight: 600;
  }
</style>
