<!-- The Definition peek (#438), under the line of the word the user picked: a header with the symbol, the
     first hit's path and line, the commit it is at, how many hits there are and ✕, and a line under it.
     That line is the hit's own until #439 shows its code, or else what the lookup is doing, that it found
     nothing, or why it failed. Hits come in ty's order until #440 ranks them. -->
<script lang="ts">
  import { peekWords, type Peek } from "$lib/state/peek";

  let { peek, onclose }: { peek: Peek; onclose: () => void } = $props();

  const words = $derived(peekWords(peek));
</script>

<div class="header">
  <code class="symbol">{peek.symbol}</code>
  {#if words.where !== undefined}
    <span class="path">{words.where}</span>
  {/if}
  <span class="muted">{words.commit}</span>
  {#if words.count !== undefined}
    <span class="muted">{words.count}</span>
  {/if}
  <button type="button" class="close" aria-label="Close" title="Close (Escape)" onclick={onclose}>✕</button>
</div>
<div class="body">
  {#if words.hit}
    <code class="code">{words.body}</code>
  {:else}
    {words.body}
  {/if}
</div>

<style>
  .header,
  .body {
    display: flex;
    gap: 8px;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    overflow: hidden;
    white-space: nowrap;
  }

  .header {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }

  .symbol {
    font-weight: 600;
  }

  .muted {
    opacity: 0.7;
  }

  .close {
    margin-left: auto;
    padding: 0 4px;
    border: none;
    background: none;
    color: inherit;
    cursor: pointer;
  }

  .code {
    white-space: pre;
  }
</style>
