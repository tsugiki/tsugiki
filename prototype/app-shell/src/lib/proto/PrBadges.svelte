<script lang="ts">
  // PROTOTYPE — throwaway. The per-PR facts every layout shows: state, CI, review state, activity.
  import type { Pr } from "./model.svelte";
  let { pr, compact = false }: { pr: Pr; compact?: boolean } = $props();
  const ciLabel: Record<string, string> = { SUCCESS: "CI passed", FAILURE: "CI failed", PENDING: "CI running", ERROR: "CI error", EXPECTED: "CI waiting" };
</script>

<span class="badges" class:compact>
  {#if pr.state !== "OPEN"}<span class="b st-{pr.state.toLowerCase()}">{pr.state.toLowerCase()}</span>{/if}
  {#if pr.isDraft}<span class="b draft">draft</span>{/if}
  {#if pr.requested}<span class="b req" title="Review request">requested</span>{/if}
  {#if pr.kind === "bot"}
    <span class="b rv-{pr.review} via" title="Bot backport: shares the review state of #{pr.reviewVia}">{pr.review === "unreviewed" ? "○" : pr.review === "reviewed" ? "✓" : "↻"} via #{pr.reviewVia}</span>
  {:else if pr.state === "OPEN" || pr.review !== "unreviewed"}
    <span class="b rv-{pr.review}" title={pr.review}>{pr.review === "unreviewed" ? "○ unreviewed" : pr.review === "reviewed" ? "✓ reviewed" : "↻ stale"}</span>
  {/if}
  {#if pr.ci && (pr.own || pr.review !== "unreviewed" || !compact)}<span class="ci ci-{pr.ci.toLowerCase()}" title={ciLabel[pr.ci]}></span>{/if}
  {#if pr.newActivity}<span class="act" title="New activity since Last seen"></span>{/if}
</span>

<style>
  .badges { display: inline-flex; gap: 4px; align-items: center; flex-wrap: nowrap; }
  .b { font-size: 11px; line-height: 16px; padding: 0 6px; border-radius: 8px; border: 1px solid var(--line); color: var(--muted); white-space: nowrap; }
  .compact .b { padding: 0 4px; }
  .st-merged { color: var(--merged); border-color: var(--merged); }
  .st-closed { color: var(--bad); border-color: var(--bad); }
  .draft { border-style: dashed; }
  .req { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .rv-unreviewed { color: var(--warn); border-color: var(--warn); }
  .rv-reviewed { color: var(--good); border-color: var(--good); }
  .rv-stale { color: var(--bad); border-color: var(--bad); }
  .via { opacity: 0.75; }
  .ci { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
  .ci-success { background: var(--good); }
  .ci-failure, .ci-error { background: var(--bad); }
  .ci-pending, .ci-expected { background: var(--warn); }
  .act { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); display: inline-block; }
</style>
