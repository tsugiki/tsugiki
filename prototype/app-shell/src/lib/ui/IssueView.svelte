<script lang="ts">
  // PROTOTYPE — throwaway. An Issue in the Detail pane: its PRs (CPython: Origin PRs with folded
  // Backports), description, recent comments and a box to comment. Links are handled by the host.
  import type { Pr } from "$lib/core/types";
  import { issueLabel, prSectionName } from "$lib/core/conventions";
  import { wb, my, togglePin, commentOnIssue, archive, mute, muteAuthor } from "$lib/state/app.svelte";
  import { ago } from "./format";
  import StateDot from "./StateDot.svelte";
  import PrStatus from "./PrStatus.svelte";
  let { n, onPr, onAuthor }: { n: number; onPr: (n: number) => void; onAuthor: (login: string) => void } = $props();

  const i = $derived(wb.issueNode(n));
  const raw = $derived(wb.issue(n));
  const label = $derived(issueLabel(wb.repo, n));
  let showMerged = $state(false);
  let draft = $state("");

  // bedevere-bot keeps a "Linked PRs" list at the end of CPython Issue descriptions; on umbrella Issues
  // it runs to hundreds of entries and repeats the Origin PRs above, so it is folded into <details>.
  function foldLinkedPrs(html: string): string {
    const doc = new DOMParser().parseFromString(html, "text/html");
    for (const h of doc.body.querySelectorAll("h1, h2, h3, h4")) {
      const list = h.nextElementSibling;
      if (h.textContent?.trim() !== "Linked PRs" || !list || !/^(UL|OL)$/.test(list.tagName)) continue;
      const details = doc.createElement("details");
      details.className = "linked";
      const summary = doc.createElement("summary");
      summary.textContent = `Linked PRs (${list.children.length})`;
      details.append(summary, list);
      h.replaceWith(details);
    }
    return doc.body.innerHTML;
  }
  const body = $derived(raw?.bodyHTML ? foldLinkedPrs(raw.bodyHTML) : "<p><i>No description.</i></p>");
  // Backports start folded under their Origin PR.
  let unfolded = $state(new Set<number>());
  function toggle(k: number) {
    const s = new Set(unfolded);
    s.has(k) ? s.delete(k) : s.add(k);
    unfolded = s;
  }
  function send() {
    if (!draft.trim()) return;
    commentOnIssue(n, draft);
    draft = "";
  }
</script>

<div class="iv">
  <header>
    <div class="crumb">{label} · {i.state.toLowerCase()}{#if raw} · opened by {raw.author} {ago(raw.createdAt)} ago{/if}</div>
    <h2><a class="ghtitle" href={raw?.url ?? `https://github.com/${wb.repo.name}/issues/${n}`} data-web title="Open on GitHub (Ctrl+Shift+G)">{i.title}<span class="ext">↗</span></a></h2>
    {#if raw}<div class="by"><button class="author" onclick={() => onAuthor(raw.author)} title="Show {raw.author}'s Issues and PRs">{raw.author}</button> opened this {ago(raw.createdAt)} ago · {raw.comments} comment{raw.comments === 1 ? "" : "s"}{#if raw.assignees.length} · assigned to {raw.assignees.join(", ")}{/if}</div>{/if}
    <div class="meta">
      {#each i.labels as l}<span class="label">{l}</span>{/each}
      <span class="muted">{i.open} open of {i.total} PRs{#if i.attention} · <b>{i.attention} need{i.attention === 1 ? "s" : ""} attention</b>{/if}</span>
      <button onclick={() => togglePin(n)} title="Ctrl+K P">{my.pins.has(n) ? "★ Unpin" : "☆ Pin"}</button>
      <button onclick={() => archive(`i:${n}`, label)} title="E: hide until something new needs you">Archive</button>
      <button onclick={() => mute(`i:${n}`, label)} title="M: hide until you unmute it">Mute</button>
      {#if raw && raw.author !== wb.me}<button onclick={() => muteAuthor(raw.author)} title="Hide what {raw.author} opens in this Repository">Mute {raw.author}</button>{/if}
    </div>
  </header>

  {#snippet row(q: Pr, bp: boolean)}
    {@const also = q.issues.filter((x) => x !== n)}
    <button class="pr" class:bp class:att={q.attention} onclick={() => onPr(q.number)}>
      <StateDot pr={q} /><span class="tgt" title={q.baseRefName}>{q.baseRefName}</span><span class="num">#{q.number}</span>
      <span class="t">{q.short}{#if also.length} <span class="also">· also fixes {also.map((x) => issueLabel(wb.repo, x)).join(", ")}</span>{/if}</span><span class="who">{q.author}</span><PrStatus pr={q} />
    </button>
  {/snippet}

  <h3>{prSectionName(wb.repo)}</h3>
  {#if i.total}
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
  {:else}<p class="muted">No PR fixes this Issue yet.</p>{/if}

  {#if raw}
    <h3>Description</h3>
    <div class="post"><div class="md">{@html body}</div></div>
    {#if raw.thread?.length}
      <h3>Comments <span class="muted">{raw.thread.length < raw.comments ? `(last ${raw.thread.length} of ${raw.comments})` : ""}</span>{#if i.talk} <span class="need">· new reply for you</span>{/if}</h3>
      {#each raw.thread as c}
        <div class="post"><div class="who2"><b>{c.author}</b> <span class="muted">{ago(c.at)} ago</span></div><div class="md">{@html c.html}</div></div>
      {/each}
    {/if}
  {/if}
  {#each my.posted.get(n) ?? [] as c}
    <div class="post mine"><div class="who2"><b>{wb.me}</b> <span class="muted">just now · not sent: prototype</span></div><div class="md"><p>{c.body}</p></div></div>
  {/each}
  <div class="compose">
    <textarea bind:value={draft} placeholder="Comment on {label}. Ctrl+Enter posts it."
      onkeydown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); send(); } }}></textarea>
    <button onclick={send} disabled={!draft.trim()}>Comment</button>
  </div>
</div>

<style>
  .iv { font: 14px/1.5 var(--sans); max-width: 900px; }
  .crumb { font-size: 12px; color: var(--muted); }
  h2 { margin: 4px 0 2px; font-size: 18px; }
  .by { font-size: 13px; color: var(--muted); margin-bottom: 8px; }
  .author { all: unset; color: var(--fg); font-weight: 600; cursor: pointer; }
  .author:hover { color: var(--accent); text-decoration: underline; }
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
  .also { font-weight: 400; color: var(--muted); font-size: 12px; }
  .bps { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 8px; align-items: center; padding: 2px 10px 4px 18px; font-size: 12px; color: var(--muted); cursor: pointer; border-bottom: 1px solid var(--line); }
  .bps:hover { background: var(--hover); }
  .chev { width: 12px; text-align: center; }
  .bp1 { display: inline-flex; gap: 4px; align-items: center; font-family: var(--mono); font-size: 11.5px; }
  .need { color: var(--accent); font-weight: 600; }
  .md :global(details.linked) { margin: 8px 0; }
  .md :global(details.linked > summary) { cursor: pointer; font-weight: 600; color: var(--muted); }
  .md :global(details.linked[open] > summary) { color: var(--fg); }
  .legend { display: flex; gap: 10px; align-items: center; font-size: 11px; margin-top: 6px; color: var(--muted); }
  .legend span { display: inline-flex; gap: 4px; align-items: center; }
  .tgt { font: 11px var(--mono); color: var(--muted); width: 32px; flex: none; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .num { font: 12px var(--mono); color: var(--accent); flex: none; }
  .t { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .who { color: var(--muted); font-size: 12px; flex: none; }
  .more { all: unset; display: block; padding: 6px 10px; font-size: 12px; color: var(--accent); cursor: pointer; }
  .post { border: 1px solid var(--line); border-radius: 6px; margin: 8px 0; }
  .post.mine { border-color: var(--warn); }
  .post .md { padding: 4px 12px; overflow-wrap: anywhere; }
  .who2 { padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 13px; }
  .compose { display: flex; flex-direction: column; gap: 6px; align-items: flex-end; margin: 12px 0 0; }
  .compose textarea { width: 100%; min-height: 70px; box-sizing: border-box; font: 13px var(--sans); background: var(--bg); color: var(--fg); border: 1px solid var(--line); border-radius: 6px; padding: 8px; }
  .compose textarea:focus { border-color: var(--accent); outline: none; }
  .muted { color: var(--muted); font-weight: 400; }
</style>
