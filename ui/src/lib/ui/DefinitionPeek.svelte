<!-- The Definition peek (#438), under the line of the word the user picked: a header with the symbol, the
     first hit's path and line, the commit it is at, how many hits there are and ✕. Under it, the hit's code
     scrolled to its line (#439), or else a line that says what the lookup is doing, that it found nothing, or
     why it failed. A hit outside the Repository names its whole path on disk in place of a commit. -->
<script lang="ts">
  import { peekWords, type Peek } from "$lib/state/peek";
  import PeekCode from "./PeekCode.svelte";

  let { peek, onclose }: { peek: Peek; onclose: () => void } = $props();

  const words = $derived(peekWords(peek));
  const code = $derived(peek.code);
</script>

<div class="header">
  <code class="symbol">{peek.symbol}</code>
  {#if words.where !== undefined}
    <span class="path">{words.where}</span>
  {/if}
  {#if words.commit !== undefined}
    <span class="muted">{words.commit}</span>
  {/if}
  {#if words.count !== undefined}
    <span class="muted">{words.count}</span>
  {/if}
  <button type="button" class="close" aria-label="Close" title="Close (Escape)" onclick={onclose}>✕</button>
</div>
{#if code && words.hit}
  <div class="code">
    <PeekCode {code} line={words.hit.line} />
  </div>
{:else}
  <div class="body">{words.body}</div>
{/if}

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

  /* The rows under the header, which the code scrolls inside. */
  .code {
    height: calc(100% - 20px);
  }
</style>
