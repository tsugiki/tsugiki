<script lang="ts">
  // PROTOTYPE — throwaway. The status bar: one thin line along the bottom for what is app-wide and has
  // no other place. Left, where the eye spends its time: sync, connection and problems. Right: unsent
  // Pending reviews, a chord in progress or the keys for this screen, and at the far end background
  // work. It stays when the Sidebar is hidden.
  import { status, jobs, pendingReviews, refreshNow } from "$lib/state/status.svelte";
  import { goTo } from "$lib/state/app.svelte";
  import { openSettings, value } from "$lib/state/settings.svelte";
  import { chord } from "./chord.svelte";
  import { prettyKey } from "./keys";
  let { hints }: { hints: string } = $props();

  let pop = $state<"jobs" | "pending" | null>(null);
  const js = $derived(jobs());
  const pend = $derived(pendingReviews());
  const comments = $derived(pend.reduce((a, p) => a + p.comments, 0));
  const since = $derived.by(() => {
    const s = Math.round((status.now - status.lastSync) / 1000);
    return s < 60 ? "just now" : s < 3600 ? `${Math.round(s / 60)} min ago` : `${Math.round(s / 3600)} h ago`;
  });
  const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 5);
  const pct = (j: { done: number; total: number }) => Math.round((100 * j.done) / j.total);
  function openPending(p: { repo: string; pr: number }) { pop = null; goTo(p.repo, `p:${p.pr}`); }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && pop) { pop = null; e.stopPropagation(); } }} />

<footer class="sb" class:offline={status.scenario === "offline"}>
  {#if status.scenario === "token"}
    <button class="seg bad" onclick={() => openSettings("Account")} title="gh-rev can read nothing new until this is fixed. Opens Settings › Account.">✕ GitHub token expired · run <code>gh auth login</code></button>
  {:else if status.scenario === "offline"}
    <button class="seg" onclick={refreshNow} title="Showing the mirror and the cached GitHub data. Writing anything (comments, Reviews) needs the network. Click to try again.">⚠ Offline · read-only · data from {clock(status.lastSync)}</button>
  {:else if status.refreshing != null || status.scenario === "syncing"}
    <span class="seg" title="Details at the right end of the bar">⟳ Syncing…</span>
  {:else}
    <button class="seg" onclick={refreshNow} title="The Workbench refreshes every {value('sync.workbench_minutes')} min (Settings › Sync). Click to refresh now.">✓ Synced {since}</button>
  {/if}
  {#if status.scenario === "rate"}
    <button class="seg warn" onclick={() => openSettings("Sync")} title="GitHub API rate limit. gh-rev refreshes less often until it resets. Opens Settings › Sync.">API: 212 of 5,000 left · resets {clock(status.now + 23 * 60e3)} · refreshing less often</button>
  {/if}

  <span class="grow"></span>

  {#if pend.length}
    <button class="seg pend" onclick={() => (pend.length === 1 ? openPending(pend[0]) : (pop = pop === "pending" ? null : "pending"))}
      title="Draft comments not yet submitted to GitHub. Click to open {pend.length === 1 ? 'the PR' : 'one'}.">
      ✎ {pend.length === 1 ? `Pending review on #${pend[0].pr}` : `${pend.length} pending reviews`} · {comments} {comments === 1 ? "comment" : "comments"}
    </button>
  {/if}
  {#if chord.pending}
    <span class="seg chord">({prettyKey(chord.pending)}) was pressed. Waiting for the second key…</span>
  {:else}
    <span class="seg hints">{hints}</span>
  {/if}
  <!-- Background work is rarely what the user looks for, so it sits at the far right. -->
  {#if js.length}
    <button class="seg jobs" onclick={() => (pop = pop === "jobs" ? null : "jobs")} title="Background work">
      <span class="spin"></span>{js[0].label}
      <span class="meter"><span style="width: {pct(js[0])}%"></span></span>
      {#if js.length > 1}<span class="more">+{js.length - 1}</span>{/if}
    </button>
  {/if}

  {#if pop === "jobs"}
    <div class="pop right">
      {#each js as j}<div class="job"><span>{j.label}</span><span class="meter"><span style="width: {pct(j)}%"></span></span><span class="n">{j.unit === "%" ? `${j.done}%` : `${j.done} / ${j.total} ${j.unit}`}</span></div>{/each}
    </div>
  {:else if pop === "pending"}
    <div class="pop right">
      {#each pend as p}<button onclick={() => openPending(p)}><span class="mono">{p.repo}#{p.pr}</span><span class="n">{p.comments} {p.comments === 1 ? "comment" : "comments"}</span></button>{/each}
    </div>
  {/if}
</footer>

<style>
  .sb { position: relative; height: var(--status-h); display: flex; align-items: stretch; gap: 2px; padding: 0 6px; font: 12px var(--sans);
    background: var(--panel); border-top: 1px solid var(--line); color: var(--muted); box-sizing: border-box; }
  .sb.offline { background: color-mix(in srgb, var(--warn) 22%, var(--panel)); color: var(--fg); }
  .seg { all: unset; display: inline-flex; gap: 6px; align-items: center; padding: 0 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  button.seg { cursor: pointer; }
  button.seg:hover { background: var(--hover); color: var(--fg); }
  .bad { background: var(--bad); color: #fff; }
  button.bad:hover { background: var(--bad); color: #fff; filter: brightness(1.1); }
  .warn { color: var(--warn); }
  .pend { color: var(--accent); }
  .chord { color: var(--fg); background: var(--sel); }
  .hints { min-width: 0; }
  code { font: 11.5px var(--mono); }
  .grow { flex: 1; }
  .spin { width: 9px; height: 9px; border: 2px solid var(--muted); border-top-color: var(--accent); border-radius: 50%; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .meter { width: 60px; height: 4px; border-radius: 2px; background: var(--line); overflow: hidden; display: inline-block; }
  .meter span { display: block; height: 100%; background: var(--accent); }
  .more { font-size: 11px; border: 1px solid var(--line); border-radius: 8px; padding: 0 5px; }
  .pop { position: absolute; bottom: calc(var(--status-h) + 4px); z-index: 70; background: var(--bg); border: 1px solid var(--line); border-radius: 6px; box-shadow: 0 6px 24px rgb(0 0 0 / 0.25); padding: 6px; min-width: 320px; color: var(--fg); }
  .pop.right { right: 6px; }
  .job { display: grid; grid-template-columns: 1fr 60px auto; gap: 10px; align-items: center; padding: 4px 6px; }
  .pop button { all: unset; cursor: pointer; display: flex; justify-content: space-between; gap: 16px; width: 100%; box-sizing: border-box; padding: 4px 6px; border-radius: 4px; }
  .pop button:hover { background: var(--hover); }
  .mono { font-family: var(--mono); }
  .n { color: var(--muted); }
</style>
