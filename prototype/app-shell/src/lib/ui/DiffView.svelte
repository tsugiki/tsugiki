<script lang="ts">
  // PROTOTYPE — throwaway. One continuous scroll over every file of a PR: NEWS entries first,
  // Generated files collapsed last, threads inline, Outdated threads at the top of their file.
  // Click a line number to comment; Ctrl+Enter adds it to the (in-memory) Pending review.
  import { parseDiff, rowOfThread, newsText, type DiffFile, type Row } from "$lib/core/diff";
  import type { Pr, Thread } from "$lib/core/types";
  import { addComment, my, say } from "$lib/state/app.svelte";
  import { value } from "$lib/state/settings.svelte";
  import { highlightRows } from "./highlight";
  import { access, tryWrite, whyReadOnly } from "$lib/state/account.svelte";
  import ReadOnly from "./ReadOnly.svelte";

  let { pr, split = false, showNews = true }: { pr: Pr; split?: boolean; showNews?: boolean } = $props();

  const files = $derived(pr.detail ? parseDiff(pr.detail) : []);
  const html = $derived(new Map(files.flatMap((f) => { const h = highlightRows(f); return f.rows.map((r, i) => [r, h[i]] as const); })));
  let expanded = $state(new Set<string>());
  let composer = $state<{ path: string; line: number; body: string } | null>(null);
  let root: HTMLElement;

  const isOpen = (f: DiffFile) => !f.generated || !value("review.collapse_generated") || expanded.has(f.path);
  function toggle(f: DiffFile) {
    const s = new Set(expanded);
    s.has(f.path) ? s.delete(f.path) : s.add(f.path);
    expanded = s;
  }
  const threadsAt = (f: DiffFile, i: number) => f.threads.filter((t) => rowOfThread(f, t) === i);
  const draftsAt = (f: DiffFile, r: Row) => (my.pending.get(pr.number) ?? []).filter((d) => d.path === f.path && d.line === (r.n ?? r.o));

  function startComment(f: DiffFile, r: Row) {
    const line = r.n ?? r.o;
    if (line != null) composer = { path: f.path, line, body: "" };
  }
  function send() {
    // Read-only, or GitHub refuses this write: the text stays in the composer.
    if (!composer?.body.trim() || !tryWrite()) return;
    addComment(pr.number, composer.path, composer.line, composer.body);
    composer = null;
  }

  /** F7 / F8 navigation, called by the host variant's keymap. */
  export function jump(what: "change" | "thread", dir: 1 | -1) {
    const sel = what === "change" ? ".row.add:not(.add + .add), .row.del:not(.del + .del)" : ".thread";
    const els = [...root.querySelectorAll<HTMLElement>(sel)];
    const scroller = root.closest(".scroll") as HTMLElement | null;
    const top = scroller ? scroller.getBoundingClientRect().top + 60 : 60;
    const target = dir > 0 ? els.find((e) => e.getBoundingClientRect().top > top + 2) : els.reverse().find((e) => e.getBoundingClientRect().top < top - 2);
    if (target) target.scrollIntoView({ block: "start", behavior: "instant" as ScrollBehavior });
    else say(what === "change" ? "No more changes" : "No more comment threads");
  }

  // Side-by-side: pair each run of deletions with the run of additions that follows it.
  function pairs(rows: Row[]) {
    const out: { l: Row | null; r: Row | null; i: number }[] = [];
    for (let i = 0; i < rows.length; ) {
      const r = rows[i];
      if (r.kind === "del") {
        const dels: number[] = [], adds: number[] = [];
        while (rows[i]?.kind === "del") dels.push(i++);
        while (rows[i]?.kind === "add") adds.push(i++);
        for (let k = 0; k < Math.max(dels.length, adds.length); k++)
          out.push({ l: rows[dels[k]] ?? null, r: rows[adds[k]] ?? null, i: adds[k] ?? dels[k] });
      } else { out.push({ l: r.kind === "add" ? null : r, r, i }); i++; }
    }
    return out;
  }
</script>

{#snippet thread(t: Thread)}
  <div class="thread" class:resolved={t.resolved} class:outdated={t.outdated}>
    <div class="thead">
      {#if t.outdated}<span class="tag">outdated · line {t.line}</span>{/if}
      {#if t.resolved}<span class="tag">resolved</span>{/if}
      <span class="muted">{t.comments.length} comment{t.comments.length === 1 ? "" : "s"}</span>
    </div>
    {#each t.comments as c}
      <div class="tc"><b>{c.author}</b> <div class="md">{@html c.html}</div></div>
    {/each}
    <div class="tactions"><button onclick={() => say("Reply box: same as a line comment (prototype)")}>Reply</button>
      <button disabled={access() === "read"} title={access() === "read" ? `Read-only: ${whyReadOnly()}` : ""} onclick={() => tryWrite() && say(`Would ${t.resolved ? "unresolve" : "resolve"} on GitHub (prototype)`)}>{t.resolved ? "Unresolve" : "Resolve"}</button></div>
  </div>
{/snippet}

{#snippet extras(f: DiffFile, r: Row, i: number)}
  {#each threadsAt(f, i) as t}{@render thread(t)}{/each}
  {#each draftsAt(f, r) as d}
    <div class="thread draft"><span class="tag">pending</span> <b>you</b>: {d.body}</div>
  {/each}
  {#if composer && composer.path === f.path && composer.line === (r.n ?? r.o)}
    <div class="composer">
      <!-- svelte-ignore a11y_autofocus -->
      <textarea autofocus bind:value={composer.body} placeholder={access() === "read" ? `Comment on line ${composer.line}. Read-only: write it here, then copy it to GitHub. Esc cancels.` : `Comment on line ${composer.line}. Ctrl+Enter adds it to your Pending review, Esc cancels.`}
        onkeydown={(e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) { e.preventDefault(); send(); } if (e.key === "Escape") composer = null; }}></textarea>
      <div class="cactions">{#if access() !== "read"}<button onclick={send}>Add to review</button>{/if}<button onclick={() => (composer = null)}>Cancel</button>
        <button onclick={() => composer && (composer.body += "\n```suggestion\n\n```")}>Suggestion</button></div>
      {#if access() === "read"}<ReadOnly text={composer.body} url="{pr.url}/files" />{/if}
    </div>
  {/if}
{/snippet}

<div class="diff" bind:this={root}>
  {#if !pr.detail}
    <div class="empty">The snapshot has no diff for #{pr.number}. Only about a dozen PRs were fetched in full; try one marked "has diff".</div>
  {:else}
    {#if showNews}
      {#each files.filter((f) => f.news) as f}
        <div class="news"><div class="newshead">NEWS entry · {f.path.split("/").slice(-2).join("/")}</div><pre>{newsText(f)}</pre></div>
      {/each}
    {/if}
    {#each files as f (f.path)}
      <section class="file" data-path={f.path}>
        <button class="fhead" onclick={() => toggle(f)}>
          <span class="chev">{isOpen(f) ? "▾" : "▸"}</span>
          <span class="path">{f.path}</span>
          {#if f.generated}<span class="gen">generated</span>{/if}
          {#if f.threads.length + f.outdated.length}<span class="muted">💬 {f.threads.length + f.outdated.length}</span>{/if}
          <span class="add-n">+{f.adds}</span><span class="del-n">−{f.dels}</span>
        </button>
        {#if isOpen(f)}
          {#each f.outdated as t}{@render thread(t)}{/each}
          {#if split}
            {#each pairs(f.rows) as p}
              {#if p.l?.kind === "hunk"}<div class="row hunk"><span class="code">{p.l.text}</span></div>
              {:else}
                <div class="srow">
                  <div class="row {p.l?.kind ?? 'none'}"><button class="ln" data-n={p.l?.o ?? ""} onclick={() => p.l && startComment(f, p.l)} aria-label="comment"></button><span class="code">{@html p.l ? html.get(p.l) : ""}</span></div>
                  <div class="row {p.r?.kind ?? 'none'}"><button class="ln" data-n={p.r?.n ?? ""} onclick={() => p.r && startComment(f, p.r)} aria-label="comment"></button><span class="code">{@html p.r ? html.get(p.r) : ""}</span></div>
                </div>
                {@render extras(f, (p.r ?? p.l)!, p.i)}
              {/if}
            {/each}
          {:else}
            {#each f.rows as r, i}
              {#if r.kind === "hunk"}<div class="row hunk"><span class="code">{r.text}</span></div>
              {:else}
                <div class="row {r.kind}">
                  <button class="ln" data-n={r.o ?? ""} onclick={() => startComment(f, r)} aria-label="comment"></button><button class="ln" data-n={r.n ?? ""} onclick={() => startComment(f, r)} aria-label="comment"></button><span class="sign">{r.kind === "add" ? "+" : r.kind === "del" ? "−" : " "}</span><span class="code">{@html html.get(r)}</span>
                </div>
              {/if}
              {@render extras(f, r, i)}
            {/each}
          {/if}
        {:else}
          <div class="collapsed">Generated file collapsed · {f.rows.length} rows · click the header to expand</div>
        {/if}
      </section>
    {/each}
    {#if pr.detail.truncated}<div class="empty">Diff truncated in the snapshot.</div>{/if}
  {/if}
</div>

<style>
  .diff { font-family: var(--mono); font-size: var(--code-size, 12.5px); }
  .empty { padding: 24px; color: var(--muted); font-family: var(--sans); }
  .news { margin: 8px 0 12px; border: 1px solid var(--line); border-left: 3px solid var(--accent); border-radius: 4px; background: var(--panel); }
  .newshead { font: 600 12px var(--sans); padding: 6px 10px; color: var(--muted); }
  .news pre { margin: 0; padding: 4px 10px 10px; white-space: pre-wrap; font: 13px/1.5 var(--sans); }
  .file { border: 1px solid var(--line); border-radius: 4px; margin-bottom: 10px; overflow: hidden; }
  .fhead { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 8px; align-items: center; padding: 6px 10px; background: var(--panel); position: sticky; top: 0; z-index: 1; cursor: pointer; font: 12.5px var(--mono); border-bottom: 1px solid var(--line); }
  .path { font-weight: 600; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .gen { font: 11px var(--sans); color: var(--muted); border: 1px dashed var(--line); padding: 0 6px; border-radius: 8px; }
  .add-n { color: var(--good); } .del-n { color: var(--bad); }
  .collapsed { padding: 8px 12px; color: var(--muted); font-family: var(--sans); font-size: 12px; }
  .row { display: flex; line-height: 20px; white-space: pre; min-height: 20px; }
  .srow { display: grid; grid-template-columns: 1fr 1fr; }
  .srow .row { overflow: hidden; }
  .srow .row + .row { border-left: 1px solid var(--line); }
  .row.add { background: var(--add-bg); } .row.del { background: var(--del-bg); }
  .row.hunk { background: var(--hunk-bg); color: var(--muted); padding-left: 8px; }
  .row.none { background: var(--panel); }
  .ln { all: unset; width: 44px; flex: none; text-align: right; padding-right: 6px; color: var(--muted); cursor: pointer; -webkit-user-select: none; user-select: none; }
  .ln::before { content: attr(data-n); }
  .ln:hover { color: var(--accent); }
  .ln:hover::after { content: " +"; }
  .sign { width: 14px; flex: none; color: var(--muted); -webkit-user-select: none; user-select: none; }
  .code { padding-right: 16px; }
  .thread { font: 13px/1.45 var(--sans); margin: 6px 12px 6px 64px; border: 1px solid var(--line); border-radius: 6px; background: var(--bg); padding: 6px 10px; white-space: normal; }
  .thread.resolved { opacity: 0.6; }
  .thread.outdated { border-style: dashed; }
  .thread.draft { border-color: var(--warn); }
  .thead { display: flex; gap: 6px; font-size: 11px; margin-bottom: 4px; }
  .tag { font-size: 11px; color: var(--warn); border: 1px solid var(--warn); border-radius: 8px; padding: 0 6px; }
  .tc { margin: 4px 0; }
  .tc .md :global(p) { margin: 2px 0 6px; }
  .tactions, .cactions { display: flex; gap: 6px; margin-top: 4px; }
  .composer { margin: 6px 12px 6px 64px; font-family: var(--sans); }
  .composer textarea { width: 100%; min-height: 70px; font: 13px var(--sans); box-sizing: border-box; background: var(--bg); color: var(--fg); border: 1px solid var(--accent); border-radius: 4px; padding: 6px; }
  .muted { color: var(--muted); }
  button { font: 12px var(--sans); }
</style>
