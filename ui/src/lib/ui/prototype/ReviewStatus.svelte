<!-- PROTOTYPE, throwaway (#367): the user's last Review on this PR and their Local mark beside it, with the
     Local mark's toggle (Ctrl+K M). -->
<script lang="ts">
  import { eventWords, proto } from "./pending-review.svelte";

  const last = $derived(proto.submitted.at(-1));
  const head = $derived(proto.head.slice(0, 7));
  const marked = $derived(proto.localMark === head);
  const days = (at: Date) => {
    const d = Math.round((Date.now() - at.getTime()) / 86_400_000);
    return d === 0 ? "today" : d === 1 ? "yesterday" : `${String(d)} days ago`;
  };
</script>

<span class="status">
  {#if last}
    <span title="Your latest Review submitted on GitHub">
      Your Review: <b>{eventWords(last.event)}</b> at <code>{last.commit}</code>, {days(last.at)}
    </span>
  {:else}
    <span>No Review from you yet</span>
  {/if}
  {#if proto.localMark}
    <span class="mark" title="Recorded only in Tsugiki; GitHub never sees it">· Local mark at <code>{proto.localMark}</code>{proto.localMark === head ? " (the head)" : ""}</span>
  {/if}
  <button type="button" class:on={marked} onclick={() => proto.toggleMark()} title="Ctrl+K M: record a Review in Tsugiki only, at the head">
    {marked ? "✓ Local mark" : "Local mark"}
  </button>
</span>

<style>
  .status {
    display: inline-flex;
    gap: 8px;
    align-items: center;
    font: 12px system-ui, sans-serif;
  }

  code {
    font-size: 11px;
  }

  .mark {
    opacity: 0.8;
  }

  button {
    padding: 1px 8px;
    font: inherit;
    cursor: pointer;
  }

  .on {
    color: #1a7f37;
    border-color: #1a7f37;
  }
</style>
