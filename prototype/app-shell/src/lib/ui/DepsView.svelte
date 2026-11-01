<script lang="ts">
  // PROTOTYPE — throwaway. Dependency updates: a dependency bot's open PRs as one item. It needs
  // attention only when one is ready to merge or its CI failed. Merging stays on GitHub for now.
  import type { WorkItem } from "$lib/core/types";
  import { wb, archive, mute, muteAuthor } from "$lib/state/app.svelte";
  import { openInBrowser } from "$lib/platform/shell";
  import { ago } from "./format";
  import StateDot from "./StateDot.svelte";
  import PrStatus from "./PrStatus.svelte";
  let { item, onPr }: { item: WorkItem; onPr: (n: number) => void } = $props();
  const ready = $derived(item.prs.filter((p) => p.why === "deps-ready").length);
</script>

<div class="dv">
  <div class="crumb">{wb.repo.name} · {item.people.join(", ")}</div>
  <h2>Dependency updates <span class="muted">({item.prs.length} open)</span></h2>
  <div class="meta">
    <span class="muted">{ready} ready to merge · {item.prs.filter((p) => p.why === "deps-failed").length} with failed CI</span>
    <button onclick={() => openInBrowser(`https://github.com/${wb.repo.name}/pulls?q=is%3Apr+is%3Aopen+author%3Aapp%2F${item.author}`)} title="Merging is on GitHub for now">Merge on GitHub</button>
    <button onclick={() => archive("deps", "Dependency updates")}>Archive</button>
    <button onclick={() => mute("deps", "Dependency updates")}>Mute</button>
    {#if item.author}<button onclick={() => muteAuthor(item.author!)}>Mute {item.author}</button>{/if}
  </div>
  <div class="prs">
    {#each item.prs as p (p.number)}
      <button class="pr" class:att={p.attention} onclick={() => onPr(p.number)}>
        <StateDot pr={p} /><span class="num">#{p.number}</span><span class="t">{p.short}</span><span class="when">{ago(p.updatedAt)}</span><PrStatus pr={p} />
      </button>
    {/each}
  </div>
</div>

<style>
  .dv { font: 14px/1.5 var(--sans); max-width: 900px; }
  .crumb { font-size: 12px; color: var(--muted); }
  h2 { margin: 4px 0 6px; font-size: 18px; }
  .meta { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 12px; margin-bottom: 10px; }
  .prs { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
  .pr { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 8px; align-items: center; padding: 4px 10px; font-size: 13px; cursor: pointer; border-bottom: 1px solid var(--line); }
  .pr:hover { background: var(--hover); }
  .pr.att .t { font-weight: 600; }
  .pr:not(.att) .t { color: var(--muted); }
  .num { font: 12px var(--mono); color: var(--accent); flex: none; }
  .t { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .when { color: var(--muted); font-size: 12px; flex: none; }
  .muted { color: var(--muted); font-weight: 400; }
</style>
