<script lang="ts">
  // PROTOTYPE — throwaway. PR description and timeline, from GitHub's pre-rendered bodyHTML.
  import { ago, type Pr } from "./model.svelte";
  let { pr, commits = true }: { pr: Pr; commits?: boolean } = $props();

  type Item = { at: string; kind: "comment" | "review" | "commit"; author: string; html?: string; state?: string; text?: string };
  const items = $derived.by(() => {
    const d = pr.detail;
    if (!d) return [] as Item[];
    const out: Item[] = [
      ...d.comments.map((c) => ({ at: c.at, kind: "comment" as const, author: c.author, html: c.html })),
      ...d.reviews.filter((r) => r.state !== "PENDING").map((r) => ({ at: r.at, kind: "review" as const, author: r.author, html: r.html, state: r.state })),
      ...(commits ? d.commits.map((c) => ({ at: c.at, kind: "commit" as const, author: c.author, text: `${c.oid} ${c.headline}` })) : []),
    ];
    return out.sort((a, b) => a.at.localeCompare(b.at));
  });
  const reviewWord: Record<string, string> = { APPROVED: "approved", CHANGES_REQUESTED: "requested changes", COMMENTED: "reviewed", DISMISSED: "review dismissed" };
</script>

<div class="conv">
  {#if !pr.detail}
    <p class="muted">The snapshot only has the conversation for about a dozen PRs. Labels: {pr.labels.join(", ") || "none"} · {pr.comments} comments on GitHub.</p>
  {:else}
    <article class="item body">
      <div class="who"><b>{pr.author}</b> <span class="muted">opened {ago(pr.createdAt)} ago</span></div>
      <div class="md">{@html pr.detail.bodyHTML || "<p><i>No description.</i></p>"}</div>
    </article>
    {#each items as it}
      {#if it.kind === "commit"}
        <div class="commit muted">● <code>{it.text}</code> <span>{it.author} · {ago(it.at)}</span></div>
      {:else if it.kind === "review" && !it.html}
        <div class="commit"><b>{it.author}</b> <span class="rv rv-{it.state}">{reviewWord[it.state!] ?? it.state}</span> <span class="muted">{ago(it.at)}</span></div>
      {:else}
        <article class="item">
          <div class="who"><b>{it.author}</b>
            {#if it.kind === "review"}<span class="rv rv-{it.state}">{reviewWord[it.state!] ?? it.state}</span>{/if}
            <span class="muted">{ago(it.at)} ago</span></div>
          <div class="md">{@html it.html}</div>
        </article>
      {/if}
    {/each}
  {/if}
</div>

<style>
  .conv { font: 14px/1.5 var(--sans); max-width: 860px; }
  .item { border: 1px solid var(--line); border-radius: 6px; margin: 10px 0; }
  .who { padding: 6px 12px; background: var(--panel); border-bottom: 1px solid var(--line); font-size: 13px; display: flex; gap: 8px; align-items: center; }
  .md { padding: 4px 12px; overflow-wrap: anywhere; }
  .commit { font-size: 12.5px; padding: 3px 12px; }
  .commit code { font: 12px var(--mono); }
  .rv { font-size: 11px; border-radius: 8px; padding: 0 6px; border: 1px solid var(--line); }
  .rv-APPROVED { color: var(--good); border-color: var(--good); }
  .rv-CHANGES_REQUESTED { color: var(--bad); border-color: var(--bad); }
  .muted { color: var(--muted); }
</style>
