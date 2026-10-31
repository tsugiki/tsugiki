<!-- PROTOTYPE — throwaway. B: no row virtualization. One <section> per file; a file's rows are
     materialized as plain in-flow HTML when it nears the viewport and dropped when far away.
     The browser lays rows out; content-visibility skips work for offscreen-but-materialized files. -->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { K, ROW_H, COMMENT_EST_H, type LoadInfo, type Row } from "./types";

  interface Props {
    info: LoadInfo;
    rows: Row[];
    collapsed: Set<number>;
    onToggle: (file: number) => void;
    scroller?: HTMLElement;
  }
  let { info, rows, collapsed, onToggle, scroller = $bindable() }: Props = $props();

  const comments = $derived(new Set(info.comment_rows));
  const maxCols = $derived(info.files.reduce((m, f) => Math.max(m, f.max_cols), 0));

  function estimate(fi: number): number {
    const f = info.files[fi];
    let h = 0;
    for (let r = f.start + 1; r < f.end; r++) h += comments.has(r) ? COMMENT_EST_H : ROW_H;
    return h;
  }

  function fileHtml(fi: number): string {
    const f = info.files[fi];
    const parts: string[] = [];
    for (let r = f.start + 1; r < f.end; r++) {
      const row = rows[r];
      if (row.k === K.COMMENT) parts.push(`<div class="cmt-row">${row.h}</div>`);
      else parts.push(`<div class="k${row.k}"><span class="ln">${row.o || ""}</span><span class="ln">${row.n || ""}</span><span class="code">${row.h}</span></div>`);
    }
    return parts.join("");
  }

  // One observer for all file bodies; rootMargin is the "materialize" distance.
  let io: IntersectionObserver | undefined;
  const bodies = new Map<Element, { fi: number; open: boolean; live: boolean }>();

  // For the benchmark: is any open-but-unmaterialized body actually on screen?
  const visible = new Set<Element>();
  let vis: IntersectionObserver | undefined;
  (window as any).__protoBlank = () =>
    [...visible].some((e) => {
      const st = bodies.get(e);
      return st !== undefined && st.open && !st.live;
    });
  onDestroy(() => {
    delete (window as any).__protoBlank;
    io?.disconnect();
    vis?.disconnect();
  });

  function sync(el: HTMLElement, st: { fi: number; open: boolean; live: boolean }, near: boolean) {
    const want = near && st.open;
    if (want && !st.live) {
      el.style.height = "";
      el.innerHTML = fileHtml(st.fi);
      st.live = true;
    } else if (!want && st.live) {
      el.style.height = st.open ? `${el.offsetHeight}px` : "";
      el.innerHTML = "";
      st.live = false;
    } else if (!st.open) {
      el.style.height = "";
    }
  }

  function body(el: HTMLElement, p: { fi: number; open: boolean }) {
    io ??= new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          const st = bodies.get(e.target);
          if (st) sync(e.target as HTMLElement, st, e.isIntersecting);
        }
      },
      { root: scroller, rootMargin: "3000px 0px" },
    );
    vis ??= new IntersectionObserver(
      (entries) => entries.forEach((e) => (e.isIntersecting ? visible.add(e.target) : visible.delete(e.target))),
      { root: scroller },
    );
    vis.observe(el);
    const st = { ...p, live: false };
    // content-visibility needs a size for skipped bodies; `auto` remembers the last rendered one.
    el.style.containIntrinsicSize = `auto ${estimate(p.fi)}px`;
    el.style.height = p.open ? `${estimate(p.fi)}px` : "";
    bodies.set(el, st);
    io.observe(el);
    return {
      update(np: { fi: number; open: boolean }) {
        st.open = np.open;
        // Re-evaluate on the next intersection callback.
        io!.unobserve(el);
        if (st.live && !st.open) sync(el, st, false);
        if (st.open && !st.live) el.style.height = `${estimate(st.fi)}px`;
        io!.observe(el);
      },
      destroy() {
        io!.unobserve(el);
        vis!.unobserve(el);
        visible.delete(el);
        bodies.delete(el);
      },
    };
  }
</script>

<div class="scroller" bind:this={scroller}>
  <div class="inner" style:min-width="calc({maxCols}ch + 140px)">
    {#each info.files as f, fi (fi)}
      <section class="file">
        <div class="fh">
          <span class="path">{f.path}</span>
          <span class="add">+{f.adds}</span><span class="del">−{f.dels}</span>
          {#if f.generated}
            <span class="badge">generated</span>
            <button class="link" onclick={() => onToggle(fi)}>{collapsed.has(fi) ? "expand" : "collapse"}</button>
          {/if}
        </div>
        {#if collapsed.has(fi)}
          <div class="collapsed">
            <button onclick={() => onToggle(fi)}>Generated file — {f.adds + f.dels} changed lines hidden. Expand</button>
          </div>
        {/if}
        <div class="body" use:body={{ fi, open: !collapsed.has(fi) }}></div>
      </section>
    {/each}
  </div>
</div>

<style>
  .scroller {
    position: absolute;
    inset: 0;
    overflow: auto;
  }
  .body {
    content-visibility: auto;
  }
  .body :global(.cmt-row) {
    padding: 6px 0 6px 112px;
  }
</style>
