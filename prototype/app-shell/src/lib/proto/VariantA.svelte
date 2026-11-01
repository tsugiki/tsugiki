<script lang="ts">
  // PROTOTYPE — throwaway. Variant A (chosen): three panes, like a mail client.
  // Sidebar: where to look (Workbench, Built-in views, Filters, Pinned). Issue list: one item per Issue,
  // like mail. Detail pane: the selected Issue (Issue view, with its Origin PRs and folded Backports)
  // or PR (PR view, Conversation and Files tabs). Esc goes from a PR back to its Issue.
  import {
    FILTERS, VIEWS, workbench, viewCount, attentionCount, pins, pr as getPr, openPr, openIssue, togglePin, toggleMark, say, ago,
    type Scope, type IssueNode,
  } from "./model.svelte";
  import StateDot from "./StateDot.svelte";
  import PrStatus from "./PrStatus.svelte";
  import IssueView from "./IssueView.svelte";
  import DiffView from "./DiffView.svelte";
  import Conversation from "./Conversation.svelte";
  import ReviewBar from "./ReviewBar.svelte";
  import Palette from "./Palette.svelte";
  import Help from "./Help.svelte";
  import { keymap } from "./keys";

  let { start = null, startIssue = null }: { start?: number | null; startIssue?: number | null } = $props();
  let scope = $state<Scope>({ kind: "all" });
  // Selection is an Issue ("i:N", N = 0 for No issue) or a PR ("p:N").
  // svelte-ignore state_referenced_locally
  let sel = $state<string | null>(start ? `p:${start}` : startIssue ? `i:${startIssue}` : null);
  let tab = $state<"conv" | "files">("conv");
  let side = $state(true);
  let split = $state(false);
  let showQuiet = $state(false);
  let palette = $state<string | null>(null);
  let help = $state(false);
  let diff = $state<DiffView>();

  const wb = $derived(workbench(scope));
  const selKind = $derived(sel?.slice(0, 1));
  const selNum = $derived(sel ? +sel.slice(2) : 0);
  const current = $derived(selKind === "p" ? getPr(selNum) : null);
  const currentIssue = $derived(selKind === "i" ? selNum || null : current?.issue ?? null);

  // The Issue list holds Issues only, like mail items; an Issue's PRs live in its Issue view.
  let sortBy = $state<"attention" | "updated">("attention");
  const byUpdated = (a: IssueNode, b: IssueNode) => b.updatedAt.localeCompare(a.updatedAt);
  const visibleIssues = $derived.by(() => {
    const active = sortBy === "updated" ? [...wb.active].sort(byUpdated) : wb.active;
    return [...active, ...(showQuiet ? wb.quiet : []), ...(wb.noIssue ? [wb.noIssue] : [])];
  });
  const flat = $derived(visibleIssues.map((i) => i.number ?? 0));
  const scopeName = $derived(
    scope.kind === "all" ? "Workbench" : scope.kind === "pinned" ? "Pinned issues" : scope.kind === "filter" ? scope.name : VIEWS.find((v) => v.key === (scope as { view: string }).view)!.name);

  function select(key: string) {
    sel = key;
    const n = +key.slice(2);
    if (key.startsWith("p:")) openPr(n); else openIssue(n || null);
    const i = key.startsWith("p:") ? getPr(n)?.issue ?? 0 : n;
    queueMicrotask(() => document.querySelector(`[data-key="i:${i}"]`)?.scrollIntoView({ block: "nearest" }));
  }
  function move(d: number) {
    const i = flat.indexOf(currentIssue ?? (selKind === "i" ? 0 : -1));
    const n = flat[Math.max(0, Math.min(flat.length - 1, i + d))];
    if (n != null) select(`i:${n}`);
  }
  /** Esc in a PR view goes back to its Issue view. */
  function back() { if (current) select(`i:${current.issue ?? 0}`); }
  const scopeEq = (s: Scope) => JSON.stringify(s) === JSON.stringify(scope);

  const commands = $derived([
    { label: "Toggle sidebar", key: "Ctrl+B", run: () => (side = !side) },
    { label: "Unified ↔ side-by-side", key: "Ctrl+Alt+U", run: () => (split = !split) },
    { label: "Local mark on selected PR", key: "Ctrl+K M", run: () => current && toggleMark(current.number) },
    { label: "Pin or unpin Issue", key: "Ctrl+K P", run: () => togglePin(currentIssue) },
    { label: "Show quiet Issues", run: () => (showQuiet = !showQuiet) },
    ...VIEWS.map((v) => ({ label: `View: ${v.name}`, run: () => (scope = { kind: "view", view: v.key }) })),
    ...FILTERS.map((f) => ({ label: `Filter: ${f.name}`, run: () => (scope = { kind: "filter", name: f.name }) })),
  ]);
  const onKey = keymap(() => ({
    "ctrl+shift+p": () => (palette = ">"),
    "ctrl+p": () => (palette = ""),
    "?": () => (help = !help),
    escape: () => { if (palette != null || help) { palette = null; help = false; } else back(); },
    arrowdown: () => move(1),
    arrowup: () => move(-1),
    enter: () => current && (tab = tab === "conv" ? "files" : "conv"),
    "ctrl+b": () => (side = !side),
    "ctrl+alt+u": () => (split = !split),
    f7: () => { tab = "files"; diff?.jump("change", 1); },
    "shift+f7": () => { tab = "files"; diff?.jump("change", -1); },
    f8: () => { tab = "files"; diff?.jump("thread", 1); },
    "shift+f8": () => { tab = "files"; diff?.jump("thread", -1); },
    f12: () => say("Go to definition: git grep at the head commit (stub in this prototype)"),
    "alt+f12": () => say("Peek definition (stub in this prototype)"),
    "ctrl+shift+e": () => say(`Would run: code -g <Review checkout>/… for #${current?.number}`),
    "ctrl+k m": () => current && toggleMark(current.number),
    "ctrl+k p": () => togglePin(currentIssue),
  }));
</script>

<svelte:window onkeydown={onKey} />

<div class="a" class:noside={!side}>
  {#if side}
    <nav class="left">
      <div class="brand">gh-rev</div>
      {#snippet count(n: number)}{#if n}<span class="n">{n}</span>{/if}{/snippet}
      <button class="nav" class:on={scopeEq({ kind: "all" })} onclick={() => (scope = { kind: "all" })}>Workbench{@render count(attentionCount({ kind: "all" }))}</button>
      <div class="sec">Built-in views</div>
      {#each VIEWS as v}
        <button class="nav" class:on={scopeEq({ kind: "view", view: v.key })} onclick={() => (scope = { kind: "view", view: v.key })}>{v.name}{@render count(viewCount(v.key))}</button>
      {/each}
      <div class="sec">Filters</div>
      {#each FILTERS as f}
        <button class="nav" title={f.q} class:on={scopeEq({ kind: "filter", name: f.name })} onclick={() => (scope = { kind: "filter", name: f.name })}>{f.name}{@render count(attentionCount({ kind: "filter", name: f.name }))}</button>
      {/each}
      <div class="sec">Pinned</div>
      <button class="nav" class:on={scopeEq({ kind: "pinned" })} onclick={() => (scope = { kind: "pinned" })}>Pinned issues{@render count(attentionCount({ kind: "pinned" }))}</button>
      <div class="hint">Numbers: Issues that need attention (PRs, for the Built-in views).</div>
    </nav>
  {/if}

  <section class="mid">
    <header class="lhead">
      <b>{scopeName}</b>
      <span class="grow"></span>
      <button class="sort" onclick={() => (sortBy = sortBy === "attention" ? "updated" : "attention")} title="Change sort order">By {sortBy} ⇅</button>
    </header>
    <div class="list">
      {#each visibleIssues as i (i.number ?? 0)}
        {@const ikey = `i:${i.number ?? 0}`}
        <div class="issue" class:att={i.attention > 0}>
          <div class="item" class:sel={currentIssue === i.number || (!i.number && sel === ikey)}>
            <button class="ibody" data-key={ikey} onclick={() => select(ikey)} title={i.reasons.join(" · ")}>
              <span class="l1">{i.people.join("; ") || "—"}</span>
              <span class="l2"><span class="ititle"><span class="inum">{i.number ? `gh-${i.number}` : "No issue"}</span> {i.number ? i.title : "PRs without a gh- prefix"}</span><span class="cnt">({i.total})</span><span class="time">{ago(i.updatedAt)}</span></span>
              <span class="l3">{i.why.length ? i.why.slice(0, 2).join(" · ") + (i.why.length > 2 ? ` · +${i.why.length - 2} more` : "") : `${i.open} open · ${i.mergedCount} merged`}</span>
            </button>
            {#if i.number}<button class="pin" class:on={i.pinned} onclick={() => togglePin(i.number)} title="Pin or unpin (Ctrl+K P)">{i.pinned ? "★" : "☆"}</button>{/if}
          </div>
        </div>
      {/each}
      {#if wb.quiet.length}
        <button class="quietbtn" onclick={() => (showQuiet = !showQuiet)}>{showQuiet ? "Hide" : "Show"} {wb.quiet.length} quiet Issues</button>
      {/if}
    </div>
    <footer class="legend">
      <span><i class="bar"></i> needs attention</span><span>★ pinned</span><span class="muted">↑ ↓ move · Esc back to the Issue</span>
    </footer>
  </section>

  <section class="right">
    {#if selKind === "i"}
      <div class="scroll"><IssueView n={selNum || null} onPr={(n) => select(`p:${n}`)} /></div>
    {:else if current}
      <header class="prhead">
        <div class="crumb">
          {#if current.issue}<button class="link" onclick={() => select(`i:${current.issue}`)}>gh-{current.issue}</button>{:else}No issue{/if}
          › #{current.number} · {current.author} → <span class="mono">{current.baseRefName}</span>{current.kind === "bot" ? ` · bot backport of #${current.origin}` : current.kind === "manual" ? ` · manual backport of #${current.origin}` : ""} · updated {ago(current.updatedAt)} ago
        </div>
        <h2>{current.short}</h2>
        <div class="row">
          <StateDot pr={current} /><span>{current.isDraft ? "draft" : current.state.toLowerCase()}</span>
          <PrStatus pr={current} />
          <span class="muted">+{current.additions} −{current.deletions} · {current.changedFiles} files · {current.commitCount} commits</span>
        </div>
        <div class="row"><ReviewBar pr={current} /></div>
        <div class="tabs">
          <button class:on={tab === "conv"} onclick={() => (tab = "conv")}>Conversation</button>
          <button class:on={tab === "files"} onclick={() => (tab = "files")}>Files changed {current.changedFiles}</button>
          <span class="muted keys">Enter switches tab · F7 / F8 jump</span>
        </div>
      </header>
      <div class="scroll">
        {#if tab === "conv"}<Conversation pr={current} />{:else}<DiffView bind:this={diff} pr={current} {split} />{/if}
      </div>
    {:else}
      <div class="placeholder">Pick an Issue or a PR (↑ ↓). Ctrl+P opens anything. ? lists keys.</div>
    {/if}
  </section>
</div>

<Palette bind:text={palette} {commands} files={current?.detail?.files.map((f) => f.path) ?? []} onPr={(n) => select(`p:${n}`)} onIssue={(n) => select(`i:${n}`)} onFile={() => (tab = "files")} />
<Help bind:open={help} extra={[["↑ ↓", "Move through Issues and PRs in the Issue list"], ["Enter", "Switch Conversation / Files"]]} />

<style>
  .a { display: grid; grid-template-columns: 220px 420px 1fr; height: 100vh; }
  .a.noside { grid-template-columns: 420px 1fr; }
  .left { border-right: 1px solid var(--line); background: var(--panel); padding: 8px; overflow: auto; display: flex; flex-direction: column; gap: 1px; }
  .brand { font-weight: 700; padding: 4px 8px 10px; }
  .sec { font-size: 11px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); padding: 12px 8px 4px; }
  .nav { all: unset; display: flex; justify-content: space-between; padding: 4px 8px; border-radius: 4px; cursor: pointer; font-size: 13px; }
  .nav:hover { background: var(--hover); } .nav.on { background: var(--sel); font-weight: 600; }
  .n { color: var(--muted); font-weight: 400; }
  .mid { border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; }
  .list { flex: 1; overflow: auto; padding-bottom: 60px; }
  .lhead { display: flex; align-items: center; gap: 8px; padding: 8px 10px; border-bottom: 1px solid var(--line); font-size: 14px; }
  .grow { flex: 1; }
  .sort { all: unset; font-size: 12px; color: var(--muted); cursor: pointer; }
  .hint { font-size: 11px; color: var(--muted); padding: 14px 8px; }
  .issue { border-bottom: 1px solid var(--line); border-left: 3px solid transparent; }
  .issue.att { border-left-color: var(--accent); }
  .item { display: flex; align-items: flex-start; position: relative; }
  .item.sel { background: var(--sel); }
  .item:hover { background: var(--hover); } .item.sel:hover { background: var(--sel); }
  .ibody { all: unset; flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; padding: 7px 10px 7px 12px; cursor: pointer; }
  .l1 { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-right: 22px; }
  .l2 { display: flex; gap: 6px; align-items: baseline; font-size: 13px; }
  .ititle { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .att .ititle { color: var(--accent); font-weight: 600; }
  .att .l1 { font-weight: 600; }
  .cnt { color: var(--muted); font-size: 12px; flex: none; }
  .time { color: var(--muted); font-size: 12px; flex: none; }
  .att .time { color: var(--accent); }
  .l3 { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .inum { font: 12px var(--mono); }
  .pin { all: unset; cursor: pointer; color: var(--muted); position: absolute; right: 10px; top: 6px; } .pin.on { color: var(--warn); }
  .bar { display: inline-block; width: 3px; height: 10px; background: var(--accent); vertical-align: middle; }
  .quietbtn { all: unset; display: block; padding: 10px; font-size: 12px; color: var(--muted); cursor: pointer; }
  .legend { display: flex; gap: 10px; align-items: center; padding: 5px 10px; border-top: 1px solid var(--line); font-size: 11px; color: var(--fg); background: var(--panel); flex-wrap: wrap; }
  .legend span { display: inline-flex; gap: 4px; align-items: center; }
  .right { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .prhead { padding: 10px 16px 0; border-bottom: 1px solid var(--line); }
  .crumb { font-size: 12px; color: var(--muted); }
  .link { all: unset; color: var(--accent); cursor: pointer; font-family: var(--mono); }
  .link:hover { text-decoration: underline; }
  h2 { margin: 4px 0 6px; font-size: 18px; }
  .row { display: flex; gap: 8px; align-items: center; margin-bottom: 6px; font-size: 12px; }
  .tabs { display: flex; gap: 2px; align-items: end; }
  .tabs button { all: unset; padding: 6px 12px; cursor: pointer; font-size: 13px; border-bottom: 2px solid transparent; }
  .tabs button.on { border-color: var(--accent); font-weight: 600; }
  .keys { margin-left: auto; font-size: 11px; padding-bottom: 6px; }
  .scroll { flex: 1; overflow: auto; padding: 12px 16px 60px; }
  .placeholder { margin: auto; color: var(--muted); }
  .muted { color: var(--muted); font-weight: 400; }
  .mono { font-family: var(--mono); }
</style>
