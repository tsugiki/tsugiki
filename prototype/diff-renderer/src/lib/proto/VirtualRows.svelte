<!-- PROTOTYPE — throwaway. Row-level virtual list used by variants A and C. -->
<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { Fenwick } from "./fenwick";
  import { buildView, estimateHeight } from "./view";
  import { K, type LoadInfo, type Row } from "./types";

  interface Props {
    info: LoadInfo;
    collapsed: Set<number>;
    onToggle: (file: number) => void;
    getRow: (id: number) => Row | undefined;
    need?: (first: number, last: number) => void;
    dataVersion?: number;
    scroller?: HTMLElement;
  }
  let { info, collapsed, onToggle, getRow, need, dataVersion = 0, scroller = $bindable() }: Props = $props();

  const OVERSCAN = 600;
  const fileAt = $derived(new Map(info.files.map((f, i) => [f.start, i])));
  const fileStarts = $derived(new Set(info.files.map((f) => f.start)));
  const comments = $derived(new Set(info.comment_rows));
  const maxCols = $derived(info.files.reduce((m, f) => Math.max(m, f.max_cols), 0));
  const measured = new Map<number, number>();

  const view = $derived(buildView(info.files, collapsed));
  let heights = new Float64Array(0);
  let fen = new Fenwick(heights);
  let layoutVersion = $state(0);

  $effect.pre(() => {
    const v = view;
    heights = new Float64Array(v.length);
    for (let i = 0; i < v.length; i++) heights[i] = measured.get(v[i]) ?? estimateHeight(v[i], fileStarts, comments);
    fen = new Fenwick(heights);
    untrack(() => layoutVersion++);
  });

  const totalH = $derived.by(() => {
    layoutVersion;
    return fen.total();
  });

  let scrollTop = $state(0);
  let viewportH = $state(900);

  const windowItems = $derived.by(() => {
    layoutVersion;
    dataVersion;
    if (!view.length) return [];
    const first = fen.indexAt(Math.max(0, scrollTop - OVERSCAN));
    const last = fen.indexAt(scrollTop + viewportH + OVERSCAN);
    const items = [];
    let top = fen.prefix(first);
    for (let i = first; i <= last; i++) {
      const id = view[i];
      items.push({ vi: i, id, top, row: id >= 0 ? getRow(id) : undefined });
      top += heights[i];
    }
    return items;
  });

  $effect(() => {
    const items = windowItems;
    if (!need || !items.length) return;
    const ids = items.map((it) => it.id).filter((id) => id >= 0);
    if (ids.length) need(ids[0], ids[ids.length - 1]);
  });

  const ro = new ResizeObserver((entries) => {
    let changed = false;
    for (const e of entries) {
      const el = e.target as HTMLElement;
      const vi = Number(el.dataset.vi);
      const id = view[vi];
      if (id === undefined) continue;
      const h = e.borderBoxSize?.[0]?.blockSize ?? el.offsetHeight;
      const delta = h - heights[vi];
      if (Math.abs(delta) < 0.5) continue;
      measured.set(id, h);
      heights[vi] = h;
      fen.add(vi, delta);
      // Keep the viewport anchored when something above it changes height.
      if (scroller && fen.prefix(vi) < scroller.scrollTop) scroller.scrollTop += delta;
      changed = true;
    }
    if (changed) layoutVersion++;
  });
  onDestroy(() => ro.disconnect());

  function measure(el: HTMLElement) {
    ro.observe(el);
    return { destroy: () => ro.unobserve(el) };
  }

  function onScroll() {
    scrollTop = scroller!.scrollTop;
    viewportH = scroller!.clientHeight;
  }
</script>

<div class="scroller" bind:this={scroller} onscroll={onScroll} bind:clientHeight={viewportH}>
  <div class="spacer" style:height="{totalH}px" style:min-width="calc({maxCols}ch + 140px)">
    {#each windowItems as it (it.id)}
      {#if it.id < 0}
        {@const fi = -it.id - 1}
        <div class="r collapsed" style:top="{it.top}px">
          <button onclick={() => onToggle(fi)}>
            Generated file — {info.files[fi].adds + info.files[fi].dels} changed lines hidden. Expand
          </button>
        </div>
      {:else if fileAt.has(it.id)}
        {@const fi = fileAt.get(it.id)!}
        {@const f = info.files[fi]}
        <div class="r fh" style:top="{it.top}px">
          <span class="path">{f.path}</span>
          <span class="add">+{f.adds}</span><span class="del">−{f.dels}</span>
          {#if f.generated}
            <span class="badge">generated</span>
            <button class="link" onclick={() => onToggle(fi)}>{collapsed.has(fi) ? "expand" : "collapse"}</button>
          {/if}
        </div>
      {:else if !it.row}
        <div class="r blank" style:top="{it.top}px" style:height="{heights[it.vi]}px"></div>
      {:else if it.row.k === K.COMMENT}
        <div class="r cmt-row" style:top="{it.top}px" data-vi={it.vi} use:measure>{@html it.row.h}</div>
      {:else}
        <div class="r k{it.row.k}" style:top="{it.top}px">
          <span class="ln">{it.row.o || ""}</span><span class="ln">{it.row.n || ""}</span><span class="code">{@html it.row.h}</span>
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
  }
  .spacer {
    position: relative;
  }
  .r {
    position: absolute;
    left: 0;
    right: 0;
  }
  .cmt-row {
    padding: 6px 0 6px 112px;
  }
  .blank {
    background: repeating-linear-gradient(90deg, var(--blank) 0 104px, transparent 104px);
  }
</style>
