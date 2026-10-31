<!-- PROTOTYPE — throwaway. C: rows stay in Rust, highlighted lazily per file, fetched in chunks over IPC. -->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import VirtualRows from "./VirtualRows.svelte";
  import { recordIpc } from "./stats.svelte";
  import type { LoadInfo, Row } from "./types";

  interface Props {
    info: LoadInfo;
    collapsed: Set<number>;
    onToggle: (file: number) => void;
    scroller?: HTMLElement;
  }
  let { info, collapsed, onToggle, scroller = $bindable() }: Props = $props();

  const CHUNK = 512;
  const cache = new Map<number, Row[]>();
  const inflight = new Set<number>();
  let dataVersion = $state(0);

  function getRow(id: number): Row | undefined {
    return cache.get(Math.floor(id / CHUNK))?.[id % CHUNK];
  }

  function need(first: number, last: number) {
    // Fetch the visible chunks plus one ahead.
    for (let c = Math.floor(first / CHUNK); c <= Math.floor(last / CHUNK) + 1; c++) {
      if (c * CHUNK >= info.total_rows || cache.has(c) || inflight.has(c)) continue;
      inflight.add(c);
      const t0 = performance.now();
      invoke<Row[]>("rows", { start: c * CHUNK, end: (c + 1) * CHUNK }).then((rows) => {
        recordIpc(performance.now() - t0);
        cache.set(c, rows);
        inflight.delete(c);
        dataVersion++;
      });
    }
  }
</script>

<VirtualRows {info} {collapsed} {onToggle} {getRow} {need} {dataVersion} bind:scroller />
