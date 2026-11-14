<!-- The file view (#453): a file the PR doesn't change, read-only at the commit of the hit Go to definition
     found in it, over the PR's Files tab. The header says where the file is from, and Back returns to the
     diff under it, scrolled where the user left it. The code is the Definition peek's, scrolled so the hit's
     line sits a few rows from the top, and marked, with its lines read a chunk at a time as they show. It
     takes the focus as it opens, so the keys scroll it and not the diff under it. -->
<script lang="ts">
  import { fileViewWords, type FileView } from "$lib/state/file-view";
  import PeekCode from "./PeekCode.svelte";

  let { view, number, onback }: { view: FileView; number: number; onback: () => void } = $props();

  const words = $derived(fileViewWords(view, number));
</script>

<section
  class="file-view"
  aria-label="File view"
  {@attach (section) => {
    section.querySelector<HTMLElement>("[data-scrolls]")?.focus({ preventScroll: true });
  }}
>
  <div class="header">
    <button type="button" class="back" onclick={onback}>{words.back}</button>
    <span class="path">{view.place.path}</span>
    <span class="muted">{words.commit}</span>
    <span class="badge">Read-only</span>
  </div>
  <div class="code">
    <PeekCode code={view.code} line={view.line} />
  </div>
</section>

<style>
  /* Covers the Files tab, which stays drawn under it, so the diff keeps where it was scrolled. */
  .file-view {
    position: absolute;
    inset: 0;
    z-index: 1;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }

  .header {
    display: flex;
    flex: none;
    gap: 12px;
    align-items: center;
    padding: 2px 8px;
    overflow: hidden;
    font-size: 12px;
    white-space: nowrap;
    border-bottom: 1px solid var(--border);
  }

  .back {
    border: 0;
    background: none;
    color: var(--accent);
    cursor: pointer;
  }

  .path {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
  }

  .muted {
    color: var(--fg-muted);
  }

  .badge {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .code {
    flex: 1;
    min-height: 0;
  }
</style>
