<script lang="ts">
  // PROTOTYPE — throwaway. The Home page: the added Repositories, each with what waits on the user
  // there, and Add repository. A normal launch skips it and reopens the last Repository.
  import { REPOS, boardOf, app, openRepo, say } from "$lib/state/app.svelte";
  import { ago } from "./format";
  import Palette from "./Palette.svelte";
  import { keymap } from "./keys";

  const rows = $derived(REPOS.map((name) => ({ name, b: boardOf(name), s: boardOf(name).summary() })));
  let sel = $state(0);
  let palette = $state<string | null>(null);

  const commands = $derived(REPOS.map((r) => ({ label: `Repository: ${r}`, run: () => openRepo(r) })));
  const onKey = keymap(() => ({
    arrowdown: () => (sel = Math.min(rows.length - 1, sel + 1)),
    arrowup: () => (sel = Math.max(0, sel - 1)),
    enter: () => openRepo(rows[sel].name),
    "ctrl+shift+p": () => (palette = ">"),
    "ctrl+r": () => (palette = ">Repository: "),
  }));
  const chip = (n: number, word: string) => (n ? `${n} ${word}` : "");
</script>

<svelte:window onkeydown={onKey} />

<div class="home">
  <header><b>gh-rev</b><span class="muted">Your Repositories. ↑ ↓ Enter to open · Ctrl+R switches from anywhere.</span></header>
  <div class="list">
    {#each rows as r, i (r.name)}
      <button class="repo" class:sel={i === sel} class:att={r.s.attention > 0} onclick={() => openRepo(r.name)} onmouseenter={() => (sel = i)}>
        <span class="l1"><span class="name">{r.name}</span>{#if r.b.repo.maintained}<span class="tag">maintained</span>{/if}<span class="grow"></span><span class="muted">{app.opened[r.name] ? `opened ${ago(app.opened[r.name])} ago` : "not opened this session"}</span></span>
        <span class="l2">
          {#if r.s.attention}<b class="n">{r.s.attention === 1 ? "1 item needs you" : `${r.s.attention} items need you`}</b>{:else}<span class="muted">nothing waiting</span>{/if}
          {#each [chip(r.s.requests, "review requests"), chip(r.s.stale, "stale reviews"), chip(r.s.ci, "failed CI on yours"), chip(r.s.waiting, "PRs to land"), chip(r.s.deps, "dependency updates"), chip(r.s.talk, "new replies")].filter(Boolean) as c}<span class="c">{c}</span>{/each}
        </span>
      </button>
    {/each}
    <button class="add" onclick={() => say("Would ask for owner/name, then fetch it (first sync by size). Prototype: the Repositories come from npm run proto:snapshot.")}>+ Add repository</button>
  </div>
</div>

<Palette bind:text={palette} {commands} onPr={() => {}} onIssue={() => {}} />

<style>
  .home { max-width: 860px; margin: 0 auto; padding: 32px 24px; font: 14px var(--sans); }
  header { display: flex; gap: 12px; align-items: baseline; margin-bottom: 16px; font-size: 18px; }
  header .muted { font-size: 13px; }
  .list { border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
  .repo { all: unset; box-sizing: border-box; width: 100%; display: flex; flex-direction: column; gap: 3px; padding: 10px 14px; border-bottom: 1px solid var(--line); border-left: 3px solid transparent; cursor: pointer; }
  .repo.att { border-left-color: var(--accent); }
  .repo.sel { background: var(--sel); }
  .l1, .l2 { display: flex; gap: 8px; align-items: baseline; font-size: 13px; flex-wrap: wrap; }
  .name { font: 600 14px var(--mono); }
  .att .name { color: var(--accent); }
  .tag { font-size: 11px; border: 1px solid var(--line); border-radius: 999px; padding: 0 6px; color: var(--muted); }
  .grow { flex: 1; }
  .n { color: var(--accent); }
  .c { color: var(--muted); }
  .c::before { content: "· "; }
  .add { all: unset; display: block; padding: 10px 14px; color: var(--accent); cursor: pointer; font-size: 13px; }
  .muted { color: var(--muted); font-weight: 400; }
</style>
