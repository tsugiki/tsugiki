<script lang="ts">
  // PROTOTYPE — throwaway. Issue or PR, at a glance, coloured by state like GitHub does.
  let { kind, state, draft = false }: { kind: "issue" | "pr"; state: string; draft?: boolean } = $props();
  const s = $derived(state === "OPEN" ? (draft ? "draft" : "open") : state.toLowerCase());
</script>

<svg class="k {s}" viewBox="0 0 16 16" aria-label="{kind === 'issue' ? 'Issue' : 'PR'}, {s}">
  <title>{kind === "issue" ? "Issue" : "PR"}, {s}</title>
  {#if kind === "issue"}
    <circle cx="8" cy="8" r="6" /><circle cx="8" cy="8" r="1.4" class="fill" />
  {:else}
    <circle cx="4" cy="3.5" r="1.8" /><circle cx="4" cy="12.5" r="1.8" /><path d="M4 5.3v5.4" />
    <circle cx="12" cy="12.5" r="1.8" /><path d="M12 10.7V6.5a2 2 0 0 0-2-2H7.2M8.8 2.9 7.2 4.5l1.6 1.6" />
  {/if}
</svg>

<style>
  .k { width: 14px; height: 14px; flex: none; vertical-align: -2px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }
  .fill { fill: currentColor; stroke: none; }
  .open { color: var(--good); }
  .draft { color: var(--muted); }
  .merged { color: var(--merged); }
  .closed { color: var(--bad); }
</style>
