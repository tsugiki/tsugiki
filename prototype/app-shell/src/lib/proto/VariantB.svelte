<script lang="ts">
  // PROTOTYPE — throwaway. Variant B: tree first, drill in.
  // The Workbench is a full-width outline (Issue › Origin PR › Backports) with columns.
  // Enter opens an Issue page (its whole tree) or a PR page: one long scroll of NEWS, conversation
  // and diff. Esc goes back. A breadcrumb shows where you are.
  import {
    FILTERS, VIEWS, workbench, viewCount, filterCount, pins, pr as getPr, issueNode, openPr, openIssue,
    togglePin, toggleMark, say, ago, type Scope, type IssueNode, type Pr,
  } from "./model.svelte";
  import PrBadges from "./PrBadges.svelte";
  import DiffView from "./DiffView.svelte";
  import Conversation from "./Conversation.svelte";
  import ReviewBar from "./ReviewBar.svelte";
  import Palette from "./Palette.svelte";
  import Help from "./Help.svelte";
  import { keymap } from "./keys";

  type Page = { kind: "wb" } | { kind: "issue"; n: number | null } | { kind: "pr"; n: number };
  let { start = null }: { start?: number | null } = $props();
  // svelte-ignore state_referenced_locally
  let stack = $state<Page[]>(start ? [{ kind: "wb" }, { kind: "pr", n: start }] : [{ kind: "wb" }]);
  const page = $derived(stack[stack.length - 1]);
  let scope = $state<Scope>({ kind: "all" });
  let cursor = $state(0);
  let collapsed = $state(new Set<string>());
  let showQuiet = $state(false);
  let split = $state(false);
  let palette = $state<string | null>(null);
  let help = $state(false);
  let diff = $state<DiffView>();

  const wb = $derived(workbench(scope));

  // The outline as flat rows, so ↑ ↓ ← → work like a tree view.
  type Line =
    | { t: "issue"; key: string; i: IssueNode }
    | { t: "pr"; key: string; p: Pr; depth: number; issue: number | null }
    | { t: "fold"; key: string; n: number; count: number; depth: number }
    | { t: "quiet"; key: string };
  function linesOf(i: IssueNode, full: boolean): Line[] {
    const key = `i${i.number ?? 0}`;
    const out: Line[] = [{ t: "issue", key, i }];
    if (collapsed.has(key) && !full) return out;
    const showAll = full || collapsed.has(`u${i.number ?? 0}`);
    for (const o of showAll ? i.origins : i.live) {
      if (o.pr) out.push({ t: "pr", key: `p${o.pr.number}`, p: o.pr, depth: 1, issue: i.number });
      for (const b of o.backports) out.push({ t: "pr", key: `p${b.number}`, p: b, depth: o.pr ? 2 : 1, issue: i.number });
    }
    if (!showAll && i.mergedCount) out.push({ t: "fold", key: `f${i.number ?? 0}`, n: i.number ?? 0, count: i.mergedCount, depth: 1 });
    return out;
  }
  const lines = $derived.by((): Line[] => {
    if (page.kind === "issue") return linesOf(issueNode(page.n), true);
    const out = wb.active.flatMap((i) => linesOf(i, false));
    if (wb.noIssue) out.push(...linesOf(wb.noIssue, false));
    if (wb.quiet.length) out.push({ t: "quiet", key: "quiet" });
    if (showQuiet) out.push(...wb.quiet.flatMap((i) => linesOf(i, false)));
    return out;
  });
  $effect(() => { if (cursor >= lines.length) cursor = Math.max(0, lines.length - 1); });

  function push(p: Page) {
    if (p.kind === "pr") openPr(p.n);
    if (p.kind === "issue") openIssue(p.n);
    stack = [...stack, p];
    cursor = 0;
  }
  function back() { if (stack.length > 1) { stack = stack.slice(0, -1); } }
  function activate(l: Line | undefined) {
    if (!l) return;
    if (l.t === "issue") push({ kind: "issue", n: l.i.number });
    else if (l.t === "pr") push({ kind: "pr", n: l.p.number });
    else if (l.t === "fold") collapsed = new Set([...collapsed, `u${l.n}`]);
    else showQuiet = !showQuiet;
  }
  function fold(open: boolean) {
    const l = lines[cursor];
    if (l?.t !== "issue") return;
    const s = new Set(collapsed);
    open ? s.delete(l.key) : s.add(l.key);
    collapsed = s;
  }
  function move(d: number) {
    cursor = Math.max(0, Math.min(lines.length - 1, cursor + d));
    document.querySelector(`[data-line="${cursor}"]`)?.scrollIntoView({ block: "nearest" });
  }
  const here = $derived(page.kind === "pr" ? getPr(page.n) : null);
  const cursorPr = $derived.by(() => { const l = lines[cursor]; return l?.t === "pr" ? l.p : null; });
  const cursorIssue = $derived.by(() => { const l = lines[cursor]; return l?.t === "issue" ? l.i.number : l?.t === "pr" ? l.issue : null; });

  const scopes: { label: string; s: Scope; n?: number }[] = $derived([
    { label: "Workbench", s: { kind: "all" } },
    ...VIEWS.map((v) => ({ label: v.name, s: { kind: "view", view: v.key } as Scope, n: viewCount(v.key) })),
    ...FILTERS.map((f) => ({ label: f.name, s: { kind: "filter", name: f.name } as Scope, n: filterCount(f.name) })),
    { label: "Pinned", s: { kind: "pinned" }, n: pins.size },
  ]);
  const scopeEq = (s: Scope) => JSON.stringify(s) === JSON.stringify(scope);

  const commands = $derived([
    { label: "Back", key: "Esc", run: back },
    { label: "Unified ↔ side-by-side", key: "Ctrl+Alt+U", run: () => (split = !split) },
    { label: "Local mark", key: "Ctrl+K M", run: () => { const n = here?.number ?? cursorPr?.number; if (n) toggleMark(n); } },
    { label: "Pin or unpin Issue", key: "Ctrl+K P", run: () => togglePin(here?.issue ?? cursorIssue) },
    ...scopes.map((x) => ({ label: `Show: ${x.label}`, run: () => { stack = [{ kind: "wb" }]; scope = x.s; } })),
  ]);
  const onKey = keymap(() => ({
    "ctrl+shift+p": () => (palette = ">"),
    "ctrl+p": () => (palette = ""),
    "?": () => (help = !help),
    escape: () => { if (palette != null || help) { palette = null; help = false; } else back(); },
    ...(page.kind !== "pr"
      ? { arrowdown: () => move(1), arrowup: () => move(-1), enter: () => activate(lines[cursor]), arrowright: () => fold(true), arrowleft: () => fold(false), "ctrl+k m": () => cursorPr && toggleMark(cursorPr.number), "ctrl+k p": () => togglePin(cursorIssue) }
      : {
          f7: () => diff?.jump("change", 1), "shift+f7": () => diff?.jump("change", -1),
          f8: () => diff?.jump("thread", 1), "shift+f8": () => diff?.jump("thread", -1),
          "ctrl+k m": () => here && toggleMark(here.number), "ctrl+k p": () => togglePin(here?.issue ?? null),
          "ctrl+alt+u": () => (split = !split),
        }),
    f12: () => say("Go to definition: git grep at the head commit (stub in this prototype)"),
    "alt+f12": () => say("Peek definition (stub in this prototype)"),
    "ctrl+shift+e": () => say("Would run: code -g on the Review checkout"),
  }));
  const crumbs = $derived(stack.map((p) => (p.kind === "wb" ? "Workbench" : p.kind === "issue" ? (p.n ? `gh-${p.n}` : "No issue") : `#${p.n}`)));
</script>

<svelte:window onkeydown={onKey} />

<div class="b">
  <header class="top">
    <span class="brand">gh-rev</span>
    <nav class="crumbs">
      {#each crumbs as c, i}
        {#if i}<span class="sep">›</span>{/if}
        <button onclick={() => (stack = stack.slice(0, i + 1))} class:cur={i === crumbs.length - 1}>{c}</button>
      {/each}
    </nav>
    <span class="muted hint">Ctrl+P open · ? keys</span>
  </header>

  {#if page.kind !== "pr"}
    {#if page.kind === "wb"}
      <div class="scopes">
        {#each scopes as x}
          <button class:on={scopeEq(x.s)} onclick={() => { scope = x.s; cursor = 0; }}>{x.label}{#if x.n != null} <span class="n">{x.n}</span>{/if}</button>
        {/each}
      </div>
    {:else}
      {@const i = issueNode(page.n)}
      <div class="issuepage">
        <h1>{page.n ? `gh-${page.n}` : "No issue"} <span>{i.title}</span></h1>
        <div class="muted">{i.state.toLowerCase()} · {i.open} open of {i.total} PRs · {i.labels.join(", ")}
          {#if page.n}<button onclick={() => togglePin(page.n)}>{i.pinned ? "★ Unpin" : "☆ Pin"}</button>{/if}</div>
      </div>
    {/if}
    <div class="outline">
      <div class="cols"><span></span><span>Title</span><span>Target</span><span>Author</span><span>State</span><span>Updated</span></div>
      {#each lines as l, idx (l.key)}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="line {l.t}" class:cur={idx === cursor} data-line={idx} onclick={() => (cursor = idx)} ondblclick={() => activate(l)}>
          {#if l.t === "issue"}
            <span class="tw">{collapsed.has(l.key) && page.kind === "wb" ? "▸" : "▾"}</span>
            <span class="ititle"><b class="inum">{l.i.number ? `gh-${l.i.number}` : "No issue"}</b> {l.i.number ? l.i.title : ""}
              {#if l.i.pinned}<span class="star">★</span>{/if}
              <span class="muted small">{l.i.reasons.join(" · ")}</span></span>
            <span></span><span></span>
            <span>{#if l.i.attention}<b class="att">{l.i.attention} need attention</b>{:else}<span class="muted">quiet</span>{/if}</span>
            <span class="muted">{l.i.open}/{l.i.total} open</span>
          {:else if l.t === "pr"}
            <span></span>
            <span class="ptitle" style="padding-left:{(l.depth - 1) * 22 + 8}px">{l.depth > 1 || l.p.kind === "bot" || l.p.kind === "manual" ? "└ " : ""}<span class="num">#{l.p.number}</span> {l.p.short}{#if l.p.detail} <span class="muted small">· has diff</span>{/if}</span>
            <span class="mono">{l.p.baseRefName}</span>
            <span class="muted">{l.p.author}</span>
            <span><PrBadges pr={l.p} compact /></span>
            <span class="muted">{ago(l.p.updatedAt)}</span>
          {:else if l.t === "fold"}
            <span></span><span class="muted fold">▸ {l.count} merged PRs (Enter to unfold)</span><span></span><span></span><span></span><span></span>
          {:else}
            <span></span><span class="muted">{showQuiet ? "▾" : "▸"} {wb.quiet.length} quiet Issues: nothing needs attention</span><span></span><span></span><span></span><span></span>
          {/if}
        </div>
      {/each}
    </div>
  {:else if here}
    <div class="prpage scroll">
      <div class="sticky">
        <div class="muted small">{here.issue ? `gh-${here.issue}` : "No issue"} · #{here.number} · {here.author} → <span class="mono">{here.baseRefName}</span> · {here.kind} · updated {ago(here.updatedAt)} ago</div>
        <h1>{here.short}</h1>
        <div class="bar"><PrBadges pr={here} /><span class="muted small">+{here.additions} −{here.deletions} · {here.changedFiles} files</span><span class="grow"></span><ReviewBar pr={here} /></div>
        {#if here.detail}
          <div class="filechips">
            {#each here.detail.files as f}<button onclick={() => document.querySelector(`[data-path="${CSS.escape(f.path)}"]`)?.scrollIntoView()}>{f.path.split("/").pop()}{f.generated ? " ⚙" : ""}</button>{/each}
          </div>
        {/if}
      </div>
      <Conversation pr={here} commits={false} />
      <h2 class="files">Files changed</h2>
      <DiffView bind:this={diff} pr={here} {split} showNews={true} />
    </div>
  {/if}
</div>

<Palette bind:text={palette} {commands} files={here?.detail?.files.map((f) => f.path) ?? []} onPr={(n) => push({ kind: "pr", n })} onIssue={(n) => push({ kind: "issue", n })} onFile={(p) => document.querySelector(`[data-path="${CSS.escape(p)}"]`)?.scrollIntoView()} />
<Help bind:open={help} extra={[["↑ ↓ ← →", "Move, fold and unfold the outline"], ["Enter / double-click", "Open Issue or PR"], ["Esc", "Back"]]} />

<style>
  .b { height: 100vh; display: flex; flex-direction: column; }
  .top { display: flex; align-items: center; gap: 14px; padding: 6px 16px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .brand { font-weight: 700; }
  .crumbs { display: flex; gap: 4px; align-items: center; flex: 1; }
  .crumbs button { all: unset; cursor: pointer; font-size: 13px; color: var(--accent); }
  .crumbs button.cur { color: var(--fg); font-weight: 600; }
  .sep { color: var(--muted); }
  .hint { font-size: 12px; }
  .scopes { display: flex; gap: 4px; padding: 8px 16px; border-bottom: 1px solid var(--line); flex-wrap: wrap; }
  .scopes button { all: unset; cursor: pointer; font-size: 12.5px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--line); }
  .scopes button.on { background: var(--fg); color: var(--bg); border-color: var(--fg); }
  .n { opacity: 0.6; }
  .issuepage { padding: 12px 16px 8px; border-bottom: 1px solid var(--line); }
  .issuepage h1 { margin: 0 0 4px; font-size: 20px; } .issuepage h1 span { font-weight: 400; }
  .outline { flex: 1; overflow: auto; padding-bottom: 60px; font-size: 13px; }
  .cols, .line { display: grid; grid-template-columns: 22px minmax(300px, 1fr) 60px 150px 260px 70px; gap: 8px; align-items: center; padding: 0 16px; }
  .cols { font-size: 11px; text-transform: uppercase; color: var(--muted); padding-top: 6px; padding-bottom: 4px; border-bottom: 1px solid var(--line); position: sticky; top: 0; background: var(--bg); z-index: 1; }
  .line { min-height: 26px; cursor: default; }
  .line.issue { min-height: 32px; border-top: 1px solid var(--line); background: var(--panel); }
  .line.cur { background: var(--sel); outline: 1px solid var(--accent); outline-offset: -1px; }
  .line > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tw { color: var(--muted); }
  .inum, .num { font-family: var(--mono); color: var(--accent); }
  .num { font-size: 12px; }
  .star { color: var(--warn); }
  .att { color: var(--warn); }
  .small { font-size: 11.5px; }
  .mono { font-family: var(--mono); font-size: 12px; }
  .fold { font-size: 12px; padding-left: 8px; }
  .prpage { flex: 1; overflow: auto; padding: 0 24px 80px; }
  .sticky { position: sticky; top: 0; background: var(--bg); z-index: 3; padding: 10px 0 6px; border-bottom: 1px solid var(--line); margin-bottom: 8px; }
  .sticky h1 { margin: 2px 0 6px; font-size: 20px; }
  .bar { display: flex; gap: 10px; align-items: center; }
  .grow { flex: 1; }
  .filechips { display: flex; gap: 4px; flex-wrap: wrap; margin-top: 6px; }
  .filechips button { font: 11px var(--mono); }
  .files { font-size: 15px; margin: 24px 0 8px; }
  .muted { color: var(--muted); }
</style>
