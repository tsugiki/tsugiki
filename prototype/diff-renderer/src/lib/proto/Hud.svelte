<!-- PROTOTYPE — throwaway. Live numbers plus repeatable benchmarks. -->
<script lang="ts">
  import { onMount } from "svelte";
  import { stats, startFrameMonitor } from "./stats.svelte";
  import { autoscroll, jumps } from "./bench";
  import type { LoadInfo } from "./types";

  interface Props {
    variant: string;
    dataset: string;
    info: LoadInfo | null;
    scroller?: HTMLElement;
    onDataset: (d: string) => void;
    onExpandAll: () => void;
    onCollapseAll: () => void;
  }
  let { variant, dataset, info, scroller, onDataset, onExpandAll, onCollapseAll }: Props = $props();

  onMount(() => startFrameMonitor());

  let open = $state(true);
  const platform = navigator.userAgent.includes("Mac") ? "macOS/WKWebView" : navigator.userAgent.includes("Windows") ? "Windows/WebView2" : navigator.userAgent;

  async function run(label: string, fn: () => Promise<object>) {
    if (!scroller || stats.busy) return;
    stats.busy = label;
    const r = await fn();
    const line = `${variant} ${dataset} ${platform} ${label}: ${JSON.stringify(r)}`;
    console.log(line);
    stats.results = [line, ...stats.results];
    stats.busy = "";
  }

  const fmt = (n: number | undefined) => (n === undefined ? "–" : n.toFixed(0));
  const copy = () => navigator.clipboard.writeText(stats.results.join("\n"));
</script>

<div class="hud">
  <button class="hud-toggle" onclick={() => (open = !open)}>{open ? "▾" : "▸"} HUD</button>
  {#if open}
    <div class="grid">
      <span>dataset</span>
      <span>
        <select value={dataset} onchange={(e) => onDataset(e.currentTarget.value)}>
          <option value="big">big (b4..rc1)</option>
          <option value="huge">huge (b1..rc1)</option>
          <option value="custom">custom (env)</option>
        </select>
      </span>
      {#if info}
        <span>files / rows</span><span>{info.files.length} / {info.total_rows.toLocaleString()}</span>
        <span>comments</span><span>{info.comment_rows.length}</span>
      {/if}
      <span>fps · p95 · worst</span><span>{stats.fps} · {stats.p95.toFixed(1)} · {stats.worst.toFixed(1)} ms</span>
      <span>frames &gt;50ms (2s)</span><span>{stats.longFrames}</span>
      <span>DOM nodes</span><span>{stats.dom.toLocaleString()}</span>
      <span>JS heap</span><span>{stats.heapMB === null ? "n/a" : `${stats.heapMB.toFixed(0)} MB`}</span>
      <span>git · attr · parse</span><span>{fmt(info?.git_ms)} · {fmt(info?.attr_ms)} · {fmt(info?.parse_ms)} ms</span>
      <span>highlight (eager)</span><span>{info?.eager ? `${fmt(info.highlight_ms)} ms` : "lazy"}</span>
      <span>all_rows IPC</span><span>{stats.load.ipc_ms ? `${fmt(stats.load.ipc_ms)} ms · ${stats.payloadMB.toFixed(1)} MB` : "–"}</span>
      <span>load → first paint</span><span>{fmt(stats.load.total_ms)} ms</span>
      <span>chunk IPC n · avg · p95</span><span>{stats.ipc.count} · {stats.ipc.avg.toFixed(1)} · {stats.ipc.p95.toFixed(1)} ms</span>
    </div>
    <div class="buttons">
      <button onclick={onExpandAll}>expand generated</button>
      <button onclick={onCollapseAll}>collapse generated</button>
      <button disabled={!!stats.busy} onclick={() => run("scroll 2000px/s", () => autoscroll(scroller!, 2000))}>scroll 2k px/s</button>
      <button disabled={!!stats.busy} onclick={() => run("scroll 20000px/s", () => autoscroll(scroller!, 20000))}>fling 20k px/s</button>
      <button disabled={!!stats.busy} onclick={() => run("30 random jumps", () => jumps(scroller!))}>30 jumps</button>
      <button disabled={!stats.results.length} onclick={copy}>copy results</button>
    </div>
    {#if stats.busy}<div class="busy">running {stats.busy}…</div>{/if}
    <ol class="results">
      {#each stats.results as r}<li>{r}</li>{/each}
    </ol>
  {/if}
</div>

<style>
  .hud {
    position: fixed;
    top: 8px;
    right: 8px;
    z-index: 10;
    width: 380px;
    max-height: calc(100vh - 80px);
    overflow: auto;
    background: #111827ee;
    color: #e5e7eb;
    font: 12px/1.5 ui-monospace, monospace;
    border-radius: 8px;
    padding: 8px 10px;
    box-shadow: 0 4px 16px #0005;
  }
  .hud-toggle {
    all: unset;
    cursor: pointer;
    font-weight: bold;
  }
  .grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0 12px;
    margin-top: 6px;
  }
  .grid > span:nth-child(odd) {
    color: #9ca3af;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: 8px;
  }
  .buttons button,
  select {
    font: inherit;
  }
  .busy {
    color: #fbbf24;
    margin-top: 4px;
  }
  .results {
    margin: 6px 0 0;
    padding-left: 18px;
    word-break: break-all;
    color: #a7f3d0;
  }
</style>
