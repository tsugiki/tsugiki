<script lang="ts">
  // PROTOTYPE — throwaway. Ctrl+P quick open (Issues, PRs, files in the current PR) and
  // Ctrl+Shift+P command palette (type ">" to switch), as in VS Code.
  import { allPrs, rawIssues, shortTitle } from "./model.svelte";

  export interface Command { label: string; key?: string; run: () => void }
  let {
    text = $bindable(null),
    commands,
    files = [],
    onPr,
    onIssue,
    onFile = () => {},
  }: {
    text: string | null; commands: Command[]; files?: string[];
    onPr: (n: number) => void; onIssue: (n: number) => void; onFile?: (path: string) => void;
  } = $props();

  type Hit = { label: string; hint: string; run: () => void };
  let sel = $state(0);
  const hits = $derived.by((): Hit[] => {
    if (text == null) return [];
    if (text.startsWith(">")) {
      const q = text.slice(1).trim().toLowerCase();
      return commands.filter((c) => c.label.toLowerCase().includes(q)).map((c) => ({ label: c.label, hint: c.key ?? "", run: c.run }));
    }
    const q = text.trim().toLowerCase().replace(/^#|^gh-/, "");
    const out: Hit[] = [];
    for (const f of files) if (!q || f.toLowerCase().includes(q)) out.push({ label: f, hint: "file in this PR", run: () => onFile(f) });
    for (const i of rawIssues.values())
      if (!q || String(i.number).startsWith(q) || i.title.toLowerCase().includes(q)) out.push({ label: `gh-${i.number}  ${i.title}`, hint: "issue", run: () => onIssue(i.number) });
    for (const p of allPrs())
      if (!q || String(p.number).startsWith(q) || p.title.toLowerCase().includes(q))
        out.push({ label: `#${p.number}  ${p.baseRefName !== "main" ? `[${p.baseRefName}] ` : ""}${shortTitle(p.title)}`, hint: p.detail ? "PR · has diff" : "PR", run: () => onPr(p.number) });
    return out.slice(0, 60);
  });
  $effect(() => { hits; sel = 0; });

  function key(e: KeyboardEvent) {
    if (e.key === "Escape") { text = null; e.preventDefault(); }
    else if (e.key === "ArrowDown") { sel = Math.min(hits.length - 1, sel + 1); e.preventDefault(); }
    else if (e.key === "ArrowUp") { sel = Math.max(0, sel - 1); e.preventDefault(); }
    else if (e.key === "Enter" && hits[sel]) { const h = hits[sel]; text = null; h.run(); e.preventDefault(); }
  }
</script>

{#if text != null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (text = null)}>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="pal" onclick={(e) => e.stopPropagation()}>
      <!-- svelte-ignore a11y_autofocus -->
      <input autofocus bind:value={text} onkeydown={key} placeholder="Issue, PR number or title, file… (> for commands)" />
      <ul>
        {#each hits as h, i}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
          <li class:sel={i === sel} onclick={() => { text = null; h.run(); }} onmouseenter={() => (sel = i)}><span>{h.label}</span><span class="hint">{h.hint}</span></li>
        {:else}<li class="hint">No matches</li>{/each}
      </ul>
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.15); z-index: 50; }
  .pal { width: min(640px, calc(100vw - 32px)); margin: 60px auto 0; background: var(--bg); border: 1px solid var(--line); border-radius: 6px; box-shadow: 0 8px 30px rgb(0 0 0 / 0.25); overflow: hidden; }
  input { width: 100%; box-sizing: border-box; padding: 8px 10px; font: 14px var(--sans); border: 0; border-bottom: 1px solid var(--line); background: var(--bg); color: var(--fg); outline: none; }
  ul { list-style: none; margin: 0; padding: 4px 0; max-height: 50vh; overflow: auto; }
  li { display: flex; justify-content: space-between; gap: 12px; padding: 3px 10px; font: 13px var(--sans); cursor: pointer; white-space: nowrap; }
  li span:first-child { overflow: hidden; text-overflow: ellipsis; }
  li.sel { background: var(--sel); }
  .hint { color: var(--muted); font-size: 12px; flex: none; }
</style>
