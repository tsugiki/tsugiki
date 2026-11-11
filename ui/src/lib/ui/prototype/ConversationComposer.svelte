<!-- PROTOTYPE, throwaway (#367): the comment box at the foot of an Issue's or a PR's Conversation. Its
     comment goes out at once, never into the Pending review. The same in every variant. -->
<script lang="ts">
  import Composer from "./Composer.svelte";
  import { proto } from "./pending-review.svelte";

  let { label }: { label: string } = $props();

  const mine = $derived(
    proto.composer?.target.kind === "conversation" && proto.composer.target.label === label ? proto.composer : undefined,
  );
</script>

{#if mine}
  <Composer composer={mine} layout="plain" />
{:else}
  <button type="button" class="open" onclick={() => proto.open({ kind: "conversation", label })}>
    Comment on {label}…
  </button>
{/if}

<style>
  .open {
    width: 100%;
    margin: 12px 0;
    padding: 8px;
    text-align: left;
    font: 13px system-ui, sans-serif;
    color: inherit;
    opacity: 0.75;
    background: Canvas;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 6px;
    cursor: text;
  }
</style>
