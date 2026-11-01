<script lang="ts">
  // PROTOTYPE — throwaway. An Issue in the right pane: description, recent comments, and its whole
  // PR tree (merged ones included), each PR clickable.
  import { issueNode, rawIssues, pins, togglePin, ago, type Pr } from "./model.svelte";
  import StateDot from "./StateDot.svelte";
  import PrStatus from "./PrStatus.svelte";
  let { n, onPr }: { n: number | null; onPr: (n: number) => void } = $props();

  const i = $derived(issueNode(n));
  const raw = $derived(n ? rawIssues.get(n) : undefined);
  let showMerged = $state(false);
  // Backports start folded under their Origin PR.
  let unfolded = $state(new Set<number>());
  function toggle(n: number) {
    const s = new Set(unfolded);
    s.has(n) ? s.delete(n) : s.add(n);
    unfolded = s;
  }
</script>

<div class="iv">
  <header>
    <div class="crumb">{n ? `gh-${n}` : "No issue"} · {i.state.toLowerCase()}{#if raw} · opened by {raw.author} {ago(raw.createdAt)} ago{/if}</div>
    <h2>{n ? i.title : "PRs without a gh- prefix"}</h2>
    <div class="meta">
      {#each i.labels as l}<span class="label">{l}</span>{/each}
      <span class="muted">{i.open} open of {i.total} PRs{#if i.attention} · <b>{i.attention} need attention</b>{/if}</span>
      {#if n}<button onclick={() => togglePin(n)} title="Ctrl+K P">{pins.has(n) ? "★ Unpin" : "☆ Pin"}</button>{/if}
    </div>
  </header>

  {#snippet row(q: Pr, bp: boolean)}
    <button class="pr" class:bp class:att={q.attention} onclick={() => onPr(q.number)}>
      <StateDot pr={q} /><span class="tgt">{q.baseRefName}</span><span class="num">#{q.number}</span>
      <span class="t">{q.short}</span><span class="who">{q.author}</span><PrStatus pr={q} />
    </button>
  {/snippet}

  <h3>Origin PRs</h3>
  <div class="prs">
    {#each showMerged ? i.origins : i.live as o (o.number)}
      {#if o.pr}{@render row(o.pr, false)}
      {:else}<div class="pr missing"><span class="num">#{o.number}</span> <span class="muted">Origin PR not in the snapshot</span></div>{/if}
      {#if o.backports.length}
        {@const open = unfolded.has(o.number) || !o.pr}
        <button class="bps" onclick={() => toggle(o.number)} title={open ? "Fold Backports" : "Unfold Backports"}>
          <span class="chev">{open ? "⌄" : "›"}</span>
          {o.backports.length} backport{o.backports.length === 1 ? "" : "s"}:
          {#each o.backports as b}<span class="bp1"><StateDot pr={b} />{b.baseRefName}{b.kind === "manual" ? " (manual)" : ""}</span>{/each}
          {#if o.backports.some((b) => b.attention)}<span class="need">· needs attention</span>{/if}
        </button>
        {#if open}{#each o.backports as b (b.number)}{@render row(b, true)}{/each}{/if}
      {/if}
    {/each}
    {#if i.mergedCount}<button class="more" onclick={() => (showMerged = !showMerged)}>{showMerged ? "Hide" : "Show"} {i.mergedCount} merged PRs</button>{/if}
  </div>
  <div class="legend"><span><StateDot pr={{ state: "OPEN", isDraft: false } as Pr} />open</span><span><StateDot pr={{ state: "OPEN", isDraft: true } as Pr} />draft</span><span><StateDot pr={{ state: "MERGED", isDraft: false } as Pr} />merged</span><span><StateDot pr={{ state: "CLOSED", isDraft: false } as Pr} />closed</span><span class="muted">· bold PRs need attention</span></div>

  {#if raw}
    <h3>Description</h3>
    <div class="post"><div class="md">{@html raw.bodyHTML || "<p><i>No description.</i></p>"}</div></div>
    {#if raw.thread?.length}
      <h3>Comments <span class="muted">{raw.thread.length < raw.comments ? `(last ${raw.thread.length} of ${raw.comments})` : ""}</span></h3>
      {#each raw.thread as c}
        <div class="post"><div class="who2"><b>{c.author}</b> <span class="muted">{ago(c.at)} ago</span></div><div class="md">{@html c.html}</div></div>
      {/each}
    {/if}
  {/if}
</div>

<style>
  .iv { font: 14px/1.5 var(--sans); max-width: 900px; }
  .crumb { font-size: 12px; color: var(--muted); }
  h2 { margin: 4px 0 6px; font-size: 18px; }
  h3 { font-size: 13px; margin: 18px 0 6px; }
  .meta { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; font-size: 12px; }
  .label { border: 1px solid var(--line); border-radius: 999px; padding: 0 8px; font-size: 11.5px; }
  .prs { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; }
  .pr { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 8px; align-items: center; padding: 4px 10px; font-size: 13px; cursor: pointer; border-bottom: 1px solid var(--line); }
  .pr:hover { background: var(--hover); }
  .pr.bp { padding-left: 34px; background: var(--panel); }
  .pr.att .t { font-weight: 600; }
  .pr:not(.att) .t { color: var(--muted); }
  .pr.missing { cursor: default; }
  .bps { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 8px; align-items: center; padding: 2px 10px 4px 18px; font-size: 12px; color: var(--muted); cursor: pointer; border-bottom: 1px solid var(--line); }
  .bps:hover { background: var(--hover); }
  .chev { width: 12px; text-align: center; }
  .bp1 { display: inline-flex; gap: 4px; align-items: center; font-family: var(--mono); font-size: 11.5px; }
  .need { color: var(--accent); font-weight: 600; }
  .legend { display: flex; gap: 10px; align-items: center; font-size: 11px; margin-top: 6px; color: var(--muted); }
  .legend span { display: inline-flex; gap: 4px; align-items: center; }
  .tgt { font: 11px var(--mono); color: var(--muted); width: 32px; flex: none; }
  .num { font: 12px var(--mono); color: var(--accent); flex: none; }
  .t { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who { color: var(--muted); font-size: 12px; flex: none; }
  .more { all: unset; display: block; padding: 6px 10px; font-size: 12px; color: var(--accent); cursor: pointer; }
  .post { border: 1px solid var(--line); border-radius: 6px; margin: 8px 0; }
  .post .md { padding: 4px 12px; overflow-wrap: anywhere; }
  .who2 { padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 13px; }
  .muted { color: var(--muted); font-weight: 400; }
</style>
