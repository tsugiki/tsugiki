<!-- PROTOTYPE — throwaway. Three rendering strategies for one-scroll PR diffs, ?variant=A|B|C&data=big|huge|custom. -->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/state";
  import { goto } from "$app/navigation";
  import { tick, untrack } from "svelte";
  import { autoscroll, jumps } from "$lib/proto/bench";
  import { SvelteSet } from "svelte/reactivity";
  import VariantA from "$lib/proto/VariantA.svelte";
  import VariantB from "$lib/proto/VariantB.svelte";
  import VariantC from "$lib/proto/VariantC.svelte";
  import Hud from "$lib/proto/Hud.svelte";
  import Switcher from "$lib/proto/Switcher.svelte";
  import { stats, resetIpc } from "$lib/proto/stats.svelte";
  import type { LoadInfo, Row } from "$lib/proto/types";

  const variants = [
    { key: "A", name: "Row virtualizer · all rows in JS" },
    { key: "B", name: "File blocks · content-visibility" },
    { key: "C", name: "Row virtualizer · rows fetched from Rust" },
  ];

  const variant = $derived(page.url.searchParams.get("variant") ?? "A");
  const dataset = $derived(page.url.searchParams.get("data") ?? "big");

  let info = $state.raw<LoadInfo | null>(null);
  let rows = $state.raw<Row[]>([]);
  let error = $state("");
  let scroller = $state<HTMLElement>();
  const collapsed = new SvelteSet<number>();
  let token = 0;

  function nav(v: string, d: string) {
    goto(`?variant=${v}&data=${d}`, { replaceState: true, keepFocus: true, noScroll: true });
  }

  $effect(() => {
    load(variant, dataset);
  });

  async function load(v: string, d: string) {
    const my = ++token;
    info = null;
    rows = [];
    error = "";
    stats.load = {};
    stats.payloadMB = 0;
    resetIpc();
    const t0 = performance.now();
    try {
      const inf = await invoke<LoadInfo>("load", { dataset: d, eager: v !== "C" });
      if (my !== token) return;
      if (v !== "C") {
        const t1 = performance.now();
        const r = await invoke<Row[]>("all_rows");
        if (my !== token) return;
        stats.load.ipc_ms = performance.now() - t1;
        stats.payloadMB = r.reduce((s, x) => s + x.h.length + 24, 0) / 1e6;
        rows = r;
      }
      collapsed.clear();
      inf.files.forEach((f, i) => f.generated && collapsed.add(i));
      info = inf;
      await tick();
      requestAnimationFrame(() => {
        if (my === token) stats.load = { ...stats.load, total_ms: performance.now() - t0 };
      });
    } catch (e) {
      error = String(e);
    }
  }

  // ?autobench=1 runs every variant × dataset, logs to a file through Rust, then quits.
  const MATRIX = ["A", "B", "C"].flatMap((v) => ["big", "huge"].map((d) => [v, d]));
  const frames = (n: number) => new Promise<void>((r) => { const f = () => (--n <= 0 ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); });

  async function autobench() {
    const platform = navigator.userAgent.includes("Mac") ? "macOS/WKWebView" : "Windows/WebView2";
    const log = (label: string, r: object) => invoke("bench_log", { line: JSON.stringify({ platform, variant, dataset, label, ...r }) });
    await frames(60);
    await log("load", {
      files: info!.files.length, rows: info!.total_rows, git_ms: info!.git_ms, parse_ms: info!.parse_ms,
      highlight_ms: info!.highlight_ms, ipc_ms: stats.load.ipc_ms ?? 0, payloadMB: stats.payloadMB, first_paint_ms: stats.load.total_ms ?? 0,
      dom: document.getElementsByTagName("*").length, heapMB: stats.heapMB,
    });
    await log("scroll 2k", await autoscroll(scroller!, 2000, 10000));
    await log("fling 20k", await autoscroll(scroller!, 20000, 10000));
    await log("jumps", await jumps(scroller!));
    const t = performance.now();
    collapsed.clear();
    await frames(2);
    await log("expand generated", { ms: +(performance.now() - t).toFixed(1), dom: document.getElementsByTagName("*").length });
    await log("fling 20k expanded", await autoscroll(scroller!, 20000, 10000));
    await log("chunk ipc", stats.ipc);
    const next = MATRIX.findIndex(([v, d]) => v === variant && d === dataset) + 1;
    if (next < MATRIX.length) goto(`?variant=${MATRIX[next][0]}&data=${MATRIX[next][1]}&autobench=1`, { replaceState: true });
    else invoke("quit");
  }

  $effect(() => {
    untrack(async () => {
      if (!page.url.searchParams.get("autobench") && (await invoke<boolean>("autobench_requested")))
        goto("?variant=A&data=big&autobench=1", { replaceState: true });
    });
  });

  $effect(() => {
    if (info && scroller && page.url.searchParams.get("autobench")) untrack(() => autobench());
  });

  function toggle(fi: number) {
    if (collapsed.has(fi)) collapsed.delete(fi);
    else collapsed.add(fi);
  }
</script>

<svelte:head>
  {#if info}{@html `<style>${info.css}</style>`}{/if}
</svelte:head>

<main>
  {#if error}
    <pre class="msg err">{error}</pre>
  {:else if !info}
    <div class="msg">Loading {dataset} for variant {variant}… (git diff + parse{variant === "C" ? "" : " + eager highlight + IPC"})</div>
  {:else if variant === "B"}
    <VariantB {info} {rows} {collapsed} onToggle={toggle} bind:scroller />
  {:else if variant === "C"}
    <VariantC {info} {collapsed} onToggle={toggle} bind:scroller />
  {:else}
    <VariantA {info} {rows} {collapsed} onToggle={toggle} bind:scroller />
  {/if}
</main>

<Hud
  {variant}
  {dataset}
  {info}
  {scroller}
  onDataset={(d) => nav(variant, d)}
  onExpandAll={() => collapsed.clear()}
  onCollapseAll={() => info?.files.forEach((f, i) => f.generated && collapsed.add(i))}
/>
<Switcher {variants} current={variant} onChange={(v) => nav(v, dataset)} />

<style>
  :global(:root) {
    --bg: #ffffff;
    --fg: #1f2328;
    --muted: #59636e;
    --border: #d1d9e0;
    --add-bg: #dafbe1;
    --del-bg: #ffebe9;
    --hunk-bg: #ddf4ff;
    --fh-bg: #f6f8fa;
    --blank: #eff2f5;
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: var(--bg);
    color: var(--fg);
    overflow: hidden;
  }
  main {
    position: fixed;
    inset: 0;
    font: 12px/20px ui-monospace, "Cascadia Mono", Menlo, Consolas, monospace;
  }
  .msg {
    padding: 24px;
    font: 14px system-ui, sans-serif;
  }
  .err {
    color: #b91c1c;
    white-space: pre-wrap;
  }

  /* Row styles shared by all variants. */
  :global(.k2),
  :global(.k3),
  :global(.k4),
  :global(.k1),
  :global(.k6) {
    height: 20px;
    white-space: pre;
    overflow: hidden;
  }
  :global(.k3) {
    background: var(--add-bg);
  }
  :global(.k4) {
    background: var(--del-bg);
  }
  :global(.k1) {
    background: var(--hunk-bg);
    color: var(--muted);
    padding-left: 112px;
  }
  :global(.k6) {
    color: var(--muted);
    padding-left: 112px;
    font-style: italic;
  }
  :global(.ln) {
    display: inline-block;
    width: 48px;
    padding-right: 8px;
    text-align: right;
    color: var(--muted);
    -webkit-user-select: none; /* WKWebView ignores the unprefixed property */
    user-select: none;
  }
  /* Line numbers as generated content: never selected or copied, on either engine. */
  :global(.ln::before) {
    content: attr(data-n);
  }
  :global(.code) {
    padding-left: 8px;
  }
  :global(.fh) {
    height: 34px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px;
    background: var(--fh-bg);
    border-top: 1px solid var(--border);
    border-bottom: 1px solid var(--border);
    font: 600 13px system-ui, sans-serif;
  }
  :global(.fh .add) {
    color: #1a7f37;
  }
  :global(.fh .del) {
    color: #cf222e;
  }
  :global(.badge) {
    font-size: 11px;
    padding: 1px 6px;
    border-radius: 999px;
    background: #fff8c5;
    border: 1px solid #d4a72c;
  }
  :global(.link) {
    all: unset;
    cursor: pointer;
    color: #0969da;
    font-weight: 400;
  }
  :global(.collapsed) {
    height: 30px;
    display: flex;
    align-items: center;
    padding-left: 112px;
  }
  :global(.collapsed button) {
    all: unset;
    cursor: pointer;
    color: #0969da;
    font: 12px system-ui, sans-serif;
  }
  :global(.thread) {
    max-width: 860px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    font: 14px/1.5 system-ui, sans-serif;
    white-space: normal;
  }
  :global(.cmt) {
    padding: 8px 12px;
  }
  :global(.cmt + .cmt) {
    border-top: 1px solid var(--border);
  }
  :global(.cmt p) {
    margin: 4px 0;
  }
  :global(.cmt pre) {
    background: var(--fh-bg);
    padding: 8px;
    border-radius: 6px;
    font: 12px/1.4 ui-monospace, monospace;
  }
  :global(.who span) {
    color: var(--muted);
    font-size: 12px;
  }
</style>
