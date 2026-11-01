<script lang="ts">
  // PROTOTYPE — throwaway. Variant A (chosen): three panes, like a mail client.
  // Sidebar: where to look (Workbench, Built-in views, Filters, Pinned). Issue list: the Issue trees
  // for that. Right pane: the selected Issue (Issue view) or PR (PR view, Conversation and Files tabs).
  import {
    FILTERS, VIEWS, workbench, viewCount, filterCount, pins, pr as getPr, openPr, openIssue, togglePin, toggleMark, say, ago,
    type Scope, type IssueNode, type Pr,
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
  let unfolded = $state(new Set<number>());
  let palette = $state<string | null>(null);
  let help = $state(false);
  let diff = $state<DiffView>();

  const wb = $derived(workbench(scope));
  const selKind = $derived(sel?.slice(0, 1));
  const selNum = $derived(sel ? +sel.slice(2) : 0);
  const current = $derived(selKind === "p" ? getPr(selNum) : null);
  const currentIssue = $derived(selKind === "i" ? selNum || null : current?.issue ?? null);

  /** The rows of one Issue in the Issue list: live trees first, all fully merged ones folded into one line. */
  function rows(i: IssueNode): (Pr | { fold: number; count: number })[] {
    const out: (Pr | { fold: number; count: number })[] = [];
    const key = i.number ?? 0;
    for (const o of unfolded.has(key) ? i.origins : i.live) out.push(...([o.pr, ...o.backports].filter(Boolean) as Pr[]));
    if (i.mergedCount && !unfolded.has(key)) out.push({ fold: key, count: i.mergedCount });
    return out;
  }
  const visibleIssues = $derived([...wb.active, ...(showQuiet ? wb.quiet : []), ...(wb.noIssue ? [wb.noIssue] : [])]);
  const flat = $derived(visibleIssues.flatMap((i) => [`i:${i.number ?? 0}`, ...rows(i).filter((r): r is Pr => !("fold" in r)).map((p) => `p:${p.number}`)]));

  function select(key: string) {
    sel = key;
    const n = +key.slice(2);
    if (key.startsWith("p:")) openPr(n); else openIssue(n || null);
    document.querySelector(`[data-key="${key}"]`)?.scrollIntoView({ block: "nearest" });
  }
  function move(d: number) {
    const i = sel == null ? -1 : flat.indexOf(sel);
    const k = flat[Math.max(0, Math.min(flat.length - 1, i + d))];
    if (k) select(k);
  }
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
    escape: () => { palette = null; help = false; },
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
      <button class="nav" class:on={scopeEq({ kind: "all" })} onclick={() => (scope = { kind: "all" })}>Workbench</button>
      <div class="sec">Built-in views</div>
      {#each VIEWS as v}
        <button class="nav" class:on={scopeEq({ kind: "view", view: v.key })} onclick={() => (scope = { kind: "view", view: v.key })}>{v.name}<span class="n">{viewCount(v.key)}</span></button>
      {/each}
      <div class="sec">Filters</div>
      {#each FILTERS as f}
        <button class="nav" title={f.q} class:on={scopeEq({ kind: "filter", name: f.name })} onclick={() => (scope = { kind: "filter", name: f.name })}>{f.name}<span class="n">{filterCount(f.name)}</span></button>
      {/each}
      <div class="sec">Pinned</div>
      <button class="nav" class:on={scopeEq({ kind: "pinned" })} onclick={() => (scope = { kind: "pinned" })}>Pinned issues<span class="n">{pins.size}</span></button>
    </nav>
  {/if}

  <section class="mid">
    <div class="list">
      {#each visibleIssues as i (i.number ?? 0)}
        {@const ikey = `i:${i.number ?? 0}`}
        <div class="issue">
          <div class="ihead" class:sel={sel === ikey}>
            <button class="iopen" data-key={ikey} onclick={() => select(ikey)}>
              <span class="inum">{i.number ? `gh-${i.number}` : "No issue"}</span>
              <span class="ititle" title={i.title}>{i.number ? i.title : "PRs without a gh- prefix"}</span>
            </button>
            {#if i.number}<button class="pin" class:on={i.pinned} onclick={() => togglePin(i.number)} title="Pin or unpin (Ctrl+K P)">{i.pinned ? "★" : "☆"}</button>{/if}
          </div>
          <div class="imeta">{#if i.attention}<b>{i.attention} need attention</b> · {/if}{i.open} open of {i.total} PRs{#if i.reasons.length} · <span class="muted">{i.reasons.join(", ")}</span>{/if}</div>
          {#each rows(i) as r}
            {#if "fold" in r}
              <button class="fold" onclick={() => (unfolded = new Set([...unfolded, r.fold]))}>▸ {r.count} merged PRs</button>
            {:else}
              <button class="prrow" class:bp={r.kind === "bot" || r.kind === "manual"} class:sel={sel === `p:${r.number}`} class:quiet={!r.attention} data-key="p:{r.number}" onclick={() => select(`p:${r.number}`)}>
                <StateDot pr={r} />
                <span class="tgt">{r.baseRefName}</span>
                <span class="t" title={r.title}>{r.short}</span>
                <PrStatus pr={r} />
              </button>
            {/if}
          {/each}
        </div>
      {/each}
      {#if wb.quiet.length}
        <button class="quietbtn" onclick={() => (showQuiet = !showQuiet)}>{showQuiet ? "Hide" : "Show"} {wb.quiet.length} quiet Issues</button>
      {/if}
    </div>
    <footer class="legend">
      <span><i class="d open"></i>open</span><span><i class="d draft"></i>draft</span><span><i class="d merged"></i>merged</span><span><i class="d closed"></i>closed</span>
      <span class="muted">· bold rows need attention</span>
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
  .issue { border-bottom: 1px solid var(--line); padding: 6px 0; }
  .ihead { display: flex; gap: 6px; align-items: center; padding: 2px 10px; }
  .ihead.sel { background: var(--sel); }
  .iopen { all: unset; display: flex; gap: 6px; align-items: baseline; flex: 1; min-width: 0; cursor: pointer; }
  .iopen:hover .ititle { text-decoration: underline; }
  .inum { font: 600 12px var(--mono); color: var(--accent); flex: none; }
  .ititle { font-weight: 600; font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pin { all: unset; cursor: pointer; color: var(--muted); } .pin.on { color: var(--warn); }
  .imeta { font-size: 11.5px; padding: 0 10px 4px; color: var(--fg); }
  .prrow { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 6px; align-items: center; padding: 3px 10px; font-size: 12.5px; cursor: pointer; font-weight: 600; }
  .prrow:hover { background: var(--hover); } .prrow.sel { background: var(--sel); }
  .prrow.bp { padding-left: 24px; }
  .prrow.quiet { font-weight: 400; }
  .prrow.quiet .t { color: var(--muted); }
  .tgt { font: 400 11px var(--mono); color: var(--muted); width: 32px; flex: none; }
  .t { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fold, .quietbtn { all: unset; display: block; padding: 2px 10px 2px 24px; font-size: 12px; color: var(--muted); cursor: pointer; }
  .quietbtn { padding: 10px; }
  .legend { display: flex; gap: 10px; align-items: center; padding: 5px 10px; border-top: 1px solid var(--line); font-size: 11px; color: var(--fg); background: var(--panel); flex-wrap: wrap; }
  .legend span { display: inline-flex; gap: 4px; align-items: center; }
  .d { width: 8px; height: 8px; border-radius: 50%; display: inline-block; box-sizing: border-box; }
  .d.open { background: var(--good); } .d.draft { border: 1.5px solid var(--muted); } .d.merged { background: var(--merged); } .d.closed { background: var(--bad); }
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
