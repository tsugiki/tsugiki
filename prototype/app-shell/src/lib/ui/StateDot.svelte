<script lang="ts">
  // PROTOTYPE — throwaway. PR state as one dot: open, draft (hollow), merged, closed.
  import type { Pr } from "$lib/core/types";
  let { pr }: { pr: Pr } = $props();
  const s = $derived(pr.state === "OPEN" ? (pr.isDraft ? "draft" : "open") : pr.state.toLowerCase());
</script>

<span class="dot {s}" title={s}></span>

<style>
  .dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; flex: none; box-sizing: border-box; }
  .open { background: var(--good); }
  .draft { border: 1.5px solid var(--muted); }
  .merged { background: var(--merged); }
  .closed { background: var(--bad); }
</style>
