<script lang="ts">
  // PROTOTYPE — throwaway. Peek: an Issue or PR elsewhere on GitHub, rendered by gh-rev from the API
  // (GitHub can't be embedded in a frame). Esc closes it.
  import type { PeekCard } from "$lib/core/types";
  import { wb, boardOf, REPOS } from "$lib/state/app.svelte";
  import { openInBrowser } from "$lib/platform/shell";
  import { ago } from "./format";

  let { target = $bindable(null), onOpenInApp }: {
    target: { repo: string; number: number; added: boolean; url: string } | null;
    onOpenInApp: (repo: string, n: number) => void;
  } = $props();

  // The snapshot keeps cards for outside links; an added Repository's own snapshot may know it too.
  function cardOf(t: NonNullable<typeof target>): PeekCard | null {
    const c = wb.peek(t.repo, t.number);
    if (c) return c;
    const name = REPOS.find((r) => r.toLowerCase() === t.repo.toLowerCase());
    if (!name) return null;
    const b = boardOf(name);
    const i = b.issue(t.number), p = b.raw(t.number);
    if (i) return { repo: name, number: t.number, kind: "issue", title: i.title, state: i.state, url: i.url, createdAt: i.createdAt, author: i.author, bodyHTML: i.bodyHTML ?? "", comments: i.comments, last: (i.thread ?? []).slice(-3) };
    if (p) return { repo: name, number: t.number, kind: "pr", title: p.title, state: p.isDraft ? "DRAFT" : p.state, url: p.url, createdAt: p.createdAt, author: p.author, bodyHTML: b.pr(t.number)?.detail?.bodyHTML ?? "", comments: p.comments, last: [] };
    return null;
  }
  const card = $derived(target ? cardOf(target) : null);
  const close = () => (target = null);
</script>

<svelte:window onkeydown={(e) => { if (target && e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); } }} />

{#if target}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={close}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="peek" onclick={(e) => e.stopPropagation()}>
      <header>
        <span class="where">{target.repo} #{target.number}</span>
        {#if card}<span class="state {card.state.toLowerCase()}">{card.state.toLowerCase()}</span>{/if}
        <span class="grow"></span>
        {#if target.added}<button onclick={() => { const t = target!; close(); onOpenInApp(t.repo, t.number); }}>Open in gh-rev</button>{/if}
        <button onclick={() => openInBrowser(card?.url ?? target!.url)}>Open in browser</button>
        <button onclick={close} title="Esc">✕</button>
      </header>
      {#if card}
        <h3>{card.title}</h3>
        <div class="by">{card.kind === "pr" ? "PR" : "Issue"} by <b>{card.author}</b> · {ago(card.createdAt)} ago · {card.comments} comment{card.comments === 1 ? "" : "s"}</div>
        <div class="scroll">
          <div class="md">{@html card.bodyHTML || "<p><i>No description.</i></p>"}</div>
          {#each card.last as c}<div class="c"><b>{c.author}</b> <span class="muted">{ago(c.at)} ago</span><div class="md">{@html c.html}</div></div>{/each}
        </div>
      {:else}
        <p class="muted">Not in this snapshot. The real app would fetch it now; here, open it in the browser.</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.2); z-index: 70; display: grid; place-items: center; }
  .peek { width: min(760px, calc(100vw - 48px)); max-height: 75vh; display: flex; flex-direction: column; background: var(--bg); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 10px 40px rgb(0 0 0 / 0.3); font: 14px/1.5 var(--sans); }
  header { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--line); font-size: 12px; }
  .where { font-family: var(--mono); color: var(--muted); }
  .state { border-radius: 999px; padding: 0 8px; color: var(--on-accent); background: var(--muted); }
  .state.open { background: var(--good); } .state.merged { background: var(--merged); } .state.closed { background: var(--bad); }
  .grow { flex: 1; }
  h3 { margin: 10px 14px 0; font-size: 16px; }
  .by { margin: 0 14px 6px; font-size: 12px; color: var(--muted); }
  .scroll { overflow: auto; padding: 0 14px 12px; }
  .c { border-top: 1px solid var(--line); padding-top: 6px; margin-top: 6px; font-size: 13px; }
  p { margin: 12px 14px; }
  .muted { color: var(--muted); }
</style>
