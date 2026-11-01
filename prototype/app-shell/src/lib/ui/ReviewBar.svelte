<script lang="ts">
  // PROTOTYPE — throwaway. Review actions for one PR. Nothing reaches GitHub.
  import type { Pr } from "$lib/core/types";
  import { my, submitReview, toggleMark } from "$lib/state/app.svelte";
  let { pr, vertical = false }: { pr: Pr; vertical?: boolean } = $props();
  const drafts = $derived(my.pending.get(pr.number)?.length ?? 0);
  const marked = $derived(my.localMarks.get(pr.number) === pr.headRefOid);
</script>

<div class="rb" class:vertical>
  <span class="muted">Pending review: <b>{drafts}</b> comment{drafts === 1 ? "" : "s"}</span>
  <button onclick={() => submitReview(pr.number, "APPROVE")} disabled={pr.own}>Approve</button>
  <button onclick={() => submitReview(pr.number, "REQUEST_CHANGES")} disabled={pr.own}>Request changes</button>
  <button onclick={() => submitReview(pr.number, "COMMENT")}>Comment</button>
  <button class:on={marked} onclick={() => toggleMark(pr.number)} title="Ctrl+K M: record a Review in gh-rev only">{marked ? "✓ Local mark" : "Local mark"}</button>
</div>

<style>
  .rb { display: flex; gap: 6px; align-items: center; font: 12px var(--sans); flex-wrap: wrap; }
  .rb.vertical { flex-direction: column; align-items: stretch; }
  .muted { color: var(--muted); }
  .on { border-color: var(--good); color: var(--good); }
</style>
