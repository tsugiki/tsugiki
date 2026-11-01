<script lang="ts">
  // PROTOTYPE — throwaway. One Repository's settings: conventions, first sync, the clone to borrow
  // objects from (this machine), Filters, Pinned issues and Muted authors (synced).
  import { CONVENTIONS, FIRST_SYNC } from "$lib/core/settings";
  import { repoConfig, setRepo, unpin, unmuteAuthorIn, removeRepo } from "$lib/state/settings.svelte";
  import Where from "./Where.svelte";
  let { name }: { name: string } = $props();
  const r = $derived(repoConfig(name));
  const setFilter = (i: number, patch: Partial<{ name: string; q: string }>) =>
    setRepo(name, { filters: r.filters.map((f, k) => (k === i ? { ...f, ...patch } : f)) });
</script>

<div class="rf">
  <div class="row"><span class="k">Conventions <Where where="synced" /></span>
    <select value={r.conventions} onchange={(e) => setRepo(name, { conventions: e.currentTarget.value as typeof r.conventions })}>
      {#each CONVENTIONS as [v, l]}<option value={v}>{l}</option>{/each}
    </select>
    <span class="help">How PRs find their Issues. Detect: CPython's rules for python/cpython, GitHub's links elsewhere.</span></div>
  <div class="row"><span class="k">First sync <Where where="synced" /></span>
    <select value={r.firstSync} onchange={(e) => setRepo(name, { firstSync: e.currentTarget.value as typeof r.firstSync })}>
      {#each FIRST_SYNC as [v, l]}<option value={v}>{l}</option>{/each}
    </select></div>
  <div class="row"><span class="k">Borrow objects from <Where where="machine" /></span>
    <input class="mono" value={r.clone} placeholder="A local clone, read only (optional)" spellcheck="false" onchange={(e) => setRepo(name, { clone: e.currentTarget.value })} />
    <span class="help">gh-rev keeps its own mirror; an existing clone saves the first download. It is never written to.</span></div>

  <div class="sub">Filters <Where where="synced" /></div>
  {#each r.filters as f, i}
    <div class="filter">
      <input class="fname" value={f.name} onchange={(e) => setFilter(i, { name: e.currentTarget.value })} />
      <input class="mono fq" value={f.q} spellcheck="false" onchange={(e) => setFilter(i, { q: e.currentTarget.value })} />
      <button onclick={() => setRepo(name, { filters: r.filters.filter((_, k) => k !== i) })} title="Remove Filter">✕</button>
    </div>
  {/each}
  <button class="add" onclick={() => setRepo(name, { filters: [...r.filters, { name: "New Filter", q: "is:open involves:@me" }] })}>+ Add Filter</button>

  <div class="sub">Pinned issues <Where where="synced" /></div>
  <div class="chips">{#each r.pinned as n}<span class="chip">#{n}<button onclick={() => unpin(name, n)} title="Unpin">✕</button></span>{:else}<span class="help">None. Ctrl+K P pins the selected Issue.</span>{/each}</div>

  <div class="sub">Muted authors <Where where="synced" /></div>
  <div class="chips">{#each r.mutedAuthors as a}<span class="chip">{a}<button onclick={() => unmuteAuthorIn(name, a)} title="Unmute">✕</button></span>{:else}<span class="help">None. "Mute author" in a PR or Issue adds one.</span>{/each}</div>

  <div class="danger"><button onclick={() => removeRepo(name)}>Remove {name}…</button></div>
</div>

<style>
  .rf { font-size: 13px; }
  .row { display: grid; grid-template-columns: 200px 1fr; gap: 2px 12px; align-items: center; margin-bottom: 10px; }
  .row .help { grid-column: 2; }
  .k { display: inline-flex; gap: 6px; align-items: center; }
  .help { color: var(--muted); font-size: 12px; }
  .sub { font-weight: 600; margin: 14px 0 6px; display: flex; gap: 6px; align-items: center; }
  input, select { font: 13px var(--sans); color: var(--fg); background: var(--bg); border: 1px solid var(--line); border-radius: 4px; padding: 3px 6px; box-sizing: border-box; }
  input:focus, select:focus { outline: none; border-color: var(--accent); }
  .mono { font-family: var(--mono); font-size: 12.5px; }
  .row input { width: 100%; }
  .filter { display: grid; grid-template-columns: 180px 1fr auto; gap: 6px; margin-bottom: 4px; }
  .add { margin-top: 2px; font-size: 12px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; }
  .chip { display: inline-flex; gap: 4px; align-items: center; border: 1px solid var(--line); border-radius: 999px; padding: 0 2px 0 8px; font: 12px var(--mono); }
  .chip button { all: unset; cursor: pointer; color: var(--muted); padding: 0 4px; }
  .chip button:hover { color: var(--bad); }
  .danger { margin-top: 18px; }
  .danger button { color: var(--bad); }
</style>
