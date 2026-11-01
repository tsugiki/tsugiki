<script lang="ts">
  // PROTOTYPE — throwaway. Variant C: a VS Code-like workspace.
  // Activity bar (Workbench / Views) · sidebar tree (Ctrl+B) · editor tabs, one per open Issue or PR
  // (Ctrl+W closes) · a bottom panel for Conversation, Pending review and Commits (Ctrl+J) ·
  // status bar. Ctrl+P is the main way around.
  import {
    ME, TAKEN_AT, FILTERS, VIEWS, workbench, viewHas, allPrs, pins, pending, localMarks, pr as getPr, issueNode,
    openPr, openIssue, togglePin, toggleMark, say, ago, type IssueNode, type Pr,
  } from "./model.svelte";
  import PrBadges from "./PrBadges.svelte";
  import DiffView from "./DiffView.svelte";
  import Conversation from "./Conversation.svelte";
  import ReviewBar from "./ReviewBar.svelte";
  import Palette from "./Palette.svelte";
  import Help from "./Help.svelte";
  import { keymap } from "./keys";

  type Tab = { kind: "issue"; n: number | null } | { kind: "pr"; n: number };
  let activity = $state<"wb" | "views" | "filters">("wb");
  let side = $state(true);
  let panel = $state(true);
  let panelTab = $state<"conv" | "review" | "commits">("conv");
  let { start = null }: { start?: number | null } = $props();
  // svelte-ignore state_referenced_locally
  let tabs = $state<Tab[]>(start ? [{ kind: "pr", n: start }] : []);
  // svelte-ignore state_referenced_locally
  let active = $state(start ? 0 : -1);
  let open = $state(new Set<string>(["sec:active", "sec:requests", "sec:stale", "sec:activity"]));
  let split = $state(false);
  let palette = $state<string | null>(null);
  let help = $state(false);
  let diff = $state<DiffView>();

  const wb = $derived(workbench({ kind: "all" }));
  const tab = $derived(tabs[active] ?? null);
  const curPr = $derived(tab?.kind === "pr" ? getPr(tab.n) : null);
  const tabKey = (t: Tab) => `${t.kind}:${t.n}`;

  function show(t: Tab) {
    const i = tabs.findIndex((x) => tabKey(x) === tabKey(t));
    if (t.kind === "pr") openPr(t.n); else openIssue(t.n);
    if (i >= 0) { active = i; return; }
    tabs = [...tabs, t];
    active = tabs.length - 1;
  }
  function close(i = active) {
    if (i < 0) return;
    tabs = tabs.filter((_, k) => k !== i);
    active = Math.min(active, tabs.length - 1);
  }
  const tog = (k: string) => { const s = new Set(open); s.has(k) ? s.delete(k) : s.add(k); open = s; };
  const totalPending = $derived([...pending.values()].reduce((a, d) => a + d.length, 0));

  const commands = $derived([
    { label: "Toggle side bar", key: "Ctrl+B", run: () => (side = !side) },
    { label: "Toggle panel", key: "Ctrl+J", run: () => (panel = !panel) },
    { label: "Close tab", key: "Ctrl+W", run: () => close() },
    { label: "Unified ↔ side-by-side", key: "Ctrl+Alt+U", run: () => (split = !split) },
    { label: "Local mark", key: "Ctrl+K M", run: () => curPr && toggleMark(curPr.number) },
    { label: "Pin or unpin Issue", key: "Ctrl+K P", run: () => togglePin(tab?.kind === "issue" ? tab.n : curPr?.issue ?? null) },
    { label: "Show Workbench", run: () => (activity = "wb") },
    { label: "Show Built-in views", run: () => (activity = "views") },
    { label: "Show Filters", run: () => (activity = "filters") },
  ]);
  const onKey = keymap(() => ({
    "ctrl+shift+p": () => (palette = ">"),
    "ctrl+p": () => (palette = ""),
    "?": () => (help = !help),
    escape: () => { palette = null; help = false; },
    "ctrl+b": () => (side = !side),
    "ctrl+j": () => (panel = !panel),
    "ctrl+w": () => close(),
    "ctrl+tab": () => tabs.length && (active = (active + 1) % tabs.length),
    "ctrl+alt+u": () => (split = !split),
    f7: () => diff?.jump("change", 1), "shift+f7": () => diff?.jump("change", -1),
    f8: () => diff?.jump("thread", 1), "shift+f8": () => diff?.jump("thread", -1),
    f12: () => say("Go to definition: git grep at the head commit (stub in this prototype)"),
    "alt+f12": () => say("Peek definition (stub in this prototype)"),
    "ctrl+shift+e": () => say("Would run: code -g on the Review checkout"),
    "ctrl+k m": () => curPr && toggleMark(curPr.number),
    "ctrl+k p": () => togglePin(tab?.kind === "issue" ? tab.n : curPr?.issue ?? null),
  }));
</script>

<svelte:window onkeydown={onKey} />

{#snippet prLeaf(p: Pr, depth: number)}
  <button class="leaf" class:on={tab?.kind === "pr" && tab.n === p.number} style="padding-left:{depth * 14 + 6}px" onclick={() => show({ kind: "pr", n: p.number })} title={p.title}>
    <span class="tgt">{p.baseRefName}</span><span class="lt" class:dim={!p.attention}>#{p.number} {p.short}</span><PrBadges pr={p} compact />
  </button>
{/snippet}

{#snippet issueBranch(i: IssueNode)}
  {@const k = `i:${i.number}`}
  <div class="branch">
    <button class="twig" onclick={() => tog(k)} ondblclick={() => show({ kind: "issue", n: i.number })}>
      <span class="tw">{open.has(k) ? "▾" : "▸"}</span>
      <span class="lt"><b class="inum">{i.number ? `gh-${i.number}` : "No issue"}</b> {i.number ? i.title : ""}</span>
      {#if i.pinned}<span class="star">★</span>{/if}
      {#if i.attention}<span class="count">{i.attention}</span>{/if}
    </button>
    {#if open.has(k)}
      {#each open.has(`m:${i.number}`) ? i.origins : i.live as o}
        {#if o.pr}{@render prLeaf(o.pr, 1)}{/if}
        {#each o.backports as b}{@render prLeaf(b, o.pr ? 2 : 1)}{/each}
      {/each}
      {#if i.mergedCount && !open.has(`m:${i.number}`)}
        <button class="leaf dim" style="padding-left:20px" onclick={() => tog(`m:${i.number}`)}>▸ {i.mergedCount} merged PRs</button>
      {/if}
    {/if}
  </div>
{/snippet}

<div class="c" class:noside={!side}>
  <nav class="act">
    <button class:on={activity === "wb"} onclick={() => { activity = "wb"; side = true; }} title="Workbench">▤</button>
    <button class:on={activity === "views"} onclick={() => { activity = "views"; side = true; }} title="Built-in views">◉</button>
    <button class:on={activity === "filters"} onclick={() => { activity = "filters"; side = true; }} title="Filters">⌕</button>
    <span class="grow"></span>
    <button onclick={() => (help = true)} title="Keys">?</button>
  </nav>

  {#if side}
    <aside class="side">
      {#if activity === "wb"}
        <div class="stitle">WORKBENCH</div>
        <button class="sec" onclick={() => tog("sec:active")}>{open.has("sec:active") ? "▾" : "▸"} NEEDS ATTENTION · {wb.active.length}</button>
        {#if open.has("sec:active")}{#each wb.active as i (i.number)}{@render issueBranch(i)}{/each}{/if}
        {#if wb.noIssue}{@render issueBranch(wb.noIssue)}{/if}
        <button class="sec" onclick={() => tog("sec:quiet")}>{open.has("sec:quiet") ? "▾" : "▸"} QUIET · {wb.quiet.length}</button>
        {#if open.has("sec:quiet")}{#each wb.quiet as i (i.number)}{@render issueBranch(i)}{/each}{/if}
      {:else if activity === "views"}
        <div class="stitle">BUILT-IN VIEWS</div>
        {#each VIEWS as v}
          {@const list = allPrs().filter((p) => viewHas(v.key, p))}
          <button class="sec" onclick={() => tog(`sec:${v.key}`)}>{open.has(`sec:${v.key}`) ? "▾" : "▸"} {v.name.toUpperCase()} · {list.length}</button>
          {#if open.has(`sec:${v.key}`)}{#each list as p}{@render prLeaf(p, 0)}{/each}{/if}
        {/each}
      {:else}
        <div class="stitle">FILTERS</div>
        {#each FILTERS as f}
          {@const res = workbench({ kind: "filter", name: f.name })}
          <button class="sec" onclick={() => tog(`f:${f.name}`)} title={f.q}>{open.has(`f:${f.name}`) ? "▾" : "▸"} {f.name.toUpperCase()} · {res.active.length + res.quiet.length}</button>
          {#if open.has(`f:${f.name}`)}
            <div class="q">{f.q}</div>
            {#each [...res.active, ...res.quiet] as i (i.number)}{@render issueBranch(i)}{/each}
            {#if res.noIssue}{@render issueBranch(res.noIssue)}{/if}
          {/if}
        {/each}
        <button class="sec" onclick={() => tog("f:pinned")}>{open.has("f:pinned") ? "▾" : "▸"} PINNED · {pins.size}</button>
        {#if open.has("f:pinned")}{#each [...pins] as n}{@render issueBranch(issueNode(n))}{/each}{/if}
      {/if}
    </aside>
  {/if}

  <main class="editor">
    <div class="tabs">
      {#each tabs as t, i (tabKey(t))}
        {@const p = t.kind === "pr" ? getPr(t.n) : null}
        <div class="tab" class:on={i === active}>
          <button onclick={() => (active = i)}>{t.kind === "pr" ? `#${t.n} ${p?.short ?? ""}` : t.n ? `gh-${t.n}` : "No issue"}</button>
          <button class="x" onclick={() => close(i)} aria-label="close tab">×</button>
        </div>
      {/each}
    </div>

    {#if !tab}
      <div class="welcome">
        <h2>gh-rev</h2>
        <p><kbd>Ctrl+P</kbd> open an Issue, PR or file · <kbd>Ctrl+Shift+P</kbd> commands · <kbd>?</kbd> keys</p>
        <p class="muted">Click a PR in the sidebar to open it in a tab. Double-click an Issue to open its overview.</p>
      </div>
    {:else if tab.kind === "issue"}
      {@const i = issueNode(tab.n)}
      <div class="body scroll">
        <h2>{tab.n ? `gh-${tab.n}` : "No issue"} <span class="light">{i.title}</span></h2>
        <p class="muted">{i.open} open of {i.total} PRs · {i.attention} need attention · {i.labels.join(", ")}
          {#if tab.n}<button onclick={() => togglePin(tab.n)}>{i.pinned ? "★ Unpin" : "☆ Pin"}</button>{/if}</p>
        <table class="grid">
          <thead><tr><th>PR</th><th>Target</th><th>Author</th><th>State</th><th>Updated</th></tr></thead>
          <tbody>
            {#each i.origins as o}
              {#each [o.pr, ...o.backports].filter(Boolean) as p}
                {@const q = p as Pr}
                <tr class:bp={q.kind === "bot" || q.kind === "manual"} onclick={() => show({ kind: "pr", n: q.number })}>
                  <td><span class="mono">#{q.number}</span> {q.short}</td><td class="mono">{q.baseRefName}</td><td>{q.author}</td>
                  <td><PrBadges pr={q} compact /></td><td class="muted">{ago(q.updatedAt)}</td>
                </tr>
              {/each}
            {/each}
          </tbody>
        </table>
      </div>
    {:else if curPr}
      <div class="prhead">
        <span class="mono muted">#{curPr.number} · {curPr.author} → {curPr.baseRefName}</span>
        <b class="ptitle">{curPr.short}</b>
        <PrBadges pr={curPr} />
      </div>
      <div class="body scroll"><DiffView bind:this={diff} pr={curPr} {split} /></div>
      {#if panel}
        <section class="panel">
          <div class="ptabs">
            <button class:on={panelTab === "conv"} onclick={() => (panelTab = "conv")}>CONVERSATION</button>
            <button class:on={panelTab === "review"} onclick={() => (panelTab = "review")}>PENDING REVIEW · {pending.get(curPr.number)?.length ?? 0}</button>
            <button class:on={panelTab === "commits"} onclick={() => (panelTab = "commits")}>COMMITS · {curPr.commitCount}</button>
            <span class="grow"></span><span class="muted">Ctrl+J</span>
          </div>
          <div class="pbody scroll">
            {#if panelTab === "conv"}<Conversation pr={curPr} commits={false} />
            {:else if panelTab === "review"}
              <ReviewBar pr={curPr} />
              {#each pending.get(curPr.number) ?? [] as d}<div class="draft"><span class="mono">{d.path}:{d.line}</span> {d.body}</div>
              {:else}<p class="muted">No draft comments. Click a line number in the diff to add one.</p>{/each}
            {:else}
              {#each curPr.detail?.commits ?? [] as c}<div class="mono small">{c.oid} {c.headline} <span class="muted">· {c.author} · {ago(c.at)}</span></div>
              {:else}<p class="muted">Not in the snapshot.</p>{/each}
            {/if}
          </div>
        </section>
      {/if}
    {/if}
  </main>

  <footer class="status">
    <span>⎇ python/cpython</span><span>{ME}</span><span>snapshot {ago(TAKEN_AT)} ago</span>
    <span class="grow"></span>
    <span>Pending review: {totalPending}</span><span>Local marks: {localMarks.size}</span><span>Pinned: {pins.size}</span>
  </footer>
</div>

<Palette bind:text={palette} {commands} files={curPr?.detail?.files.map((f) => f.path) ?? []} onPr={(n) => show({ kind: "pr", n })} onIssue={(n) => show({ kind: "issue", n })} onFile={(p) => document.querySelector(`[data-path="${CSS.escape(p)}"]`)?.scrollIntoView()} />
<Help bind:open={help} extra={[["Ctrl+J", "Toggle panel"], ["Ctrl+W / Ctrl+Tab", "Close / next tab"]]} />

<style>
  .c { display: grid; grid-template-columns: 44px 320px 1fr; grid-template-rows: 1fr 22px; height: 100vh; }
  .c.noside { grid-template-columns: 44px 1fr; }
  .act { grid-row: 1; background: var(--chrome); display: flex; flex-direction: column; align-items: center; padding-top: 6px; gap: 4px; }
  .act button { all: unset; width: 44px; height: 40px; text-align: center; font-size: 18px; color: var(--chrome-fg); opacity: 0.6; cursor: pointer; border-left: 2px solid transparent; box-sizing: border-box; }
  .act button.on { opacity: 1; border-left-color: var(--accent); }
  .side { background: var(--panel); border-right: 1px solid var(--line); overflow: auto; font-size: 13px; padding-bottom: 40px; }
  .stitle { font-size: 11px; padding: 8px 12px; color: var(--muted); letter-spacing: 0.05em; }
  .sec { all: unset; display: block; width: 100%; box-sizing: border-box; font: 700 11px var(--sans); padding: 4px 6px; cursor: pointer; border-top: 1px solid var(--line); }
  .q { font: 11px var(--mono); color: var(--muted); padding: 2px 12px 4px; overflow-wrap: anywhere; }
  .twig, .leaf { all: unset; box-sizing: border-box; width: 100%; display: flex; gap: 5px; align-items: center; padding: 2px 6px; cursor: pointer; white-space: nowrap; }
  .twig:hover, .leaf:hover { background: var(--hover); }
  .leaf.on { background: var(--sel); }
  .tw { width: 12px; color: var(--muted); flex: none; }
  .lt { flex: 1; overflow: hidden; text-overflow: ellipsis; }
  .dim { color: var(--muted); }
  .inum { font: 600 12px var(--mono); color: var(--accent); }
  .tgt { font: 10.5px var(--mono); color: var(--muted); width: 30px; flex: none; }
  .count { background: var(--accent); color: var(--on-accent); border-radius: 8px; font-size: 11px; padding: 0 6px; }
  .star { color: var(--warn); }
  .editor { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .tabs { display: flex; background: var(--panel); border-bottom: 1px solid var(--line); overflow-x: auto; flex: none; min-height: 32px; }
  .tab { display: flex; align-items: center; border-right: 1px solid var(--line); max-width: 260px; }
  .tab.on { background: var(--bg); box-shadow: inset 0 2px 0 var(--accent); }
  .tab button { all: unset; padding: 6px 4px 6px 10px; font-size: 12.5px; cursor: pointer; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tab .x { padding: 6px 8px; opacity: 0.5; }
  .welcome { margin: 15vh auto; text-align: center; color: var(--fg); }
  .prhead { display: flex; gap: 10px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--line); font-size: 13px; flex: none; }
  .ptitle { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .body { flex: 1; overflow: auto; padding: 10px 12px 60px; min-height: 0; }
  .light { font-weight: 400; }
  .panel { height: 36%; border-top: 1px solid var(--line); display: flex; flex-direction: column; flex: none; }
  .ptabs { display: flex; gap: 2px; padding: 0 8px; font-size: 11px; align-items: center; flex: none; }
  .ptabs button { all: unset; padding: 6px 10px; cursor: pointer; letter-spacing: 0.04em; border-bottom: 1px solid transparent; }
  .ptabs button.on { border-color: var(--accent); }
  .pbody { flex: 1; overflow: auto; padding: 4px 14px 50px; }
  .draft { font-size: 13px; padding: 4px 0; border-bottom: 1px solid var(--line); }
  .grid { border-collapse: collapse; width: 100%; font-size: 13px; }
  .grid th { text-align: left; font-size: 11px; color: var(--muted); border-bottom: 1px solid var(--line); padding: 4px 8px; }
  .grid td { padding: 3px 8px; border-bottom: 1px solid var(--line); white-space: nowrap; }
  .grid tr { cursor: pointer; } .grid tr:hover { background: var(--hover); }
  .grid tr.bp td:first-child { padding-left: 28px; }
  .status { grid-column: 1 / -1; background: var(--accent); color: var(--on-accent); display: flex; gap: 14px; align-items: center; padding: 0 10px; font-size: 12px; }
  .grow { flex: 1; }
  .mono { font-family: var(--mono); font-size: 12px; }
  .small { font-size: 12px; padding: 2px 0; }
  .muted { color: var(--muted); }
  kbd { font: 12px var(--mono); border: 1px solid var(--line); border-radius: 3px; padding: 0 4px; }
</style>
