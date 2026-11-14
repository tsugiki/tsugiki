<!-- The file view (#453): a file the PR doesn't change, read-only at the commit of the hit Go to definition
     found in it, over the PR's Files tab. The header says where the file is from, and Back returns to the
     diff under it, scrolled where the user left it. The code is the Definition peek's, scrolled so the hit's
     line sits a few rows from the top, and marked, with its lines read a chunk at a time as they show. It
     takes the focus as it opens, so the keys scroll it and not the diff under it. The user follows symbols
     in it as in the diff (#457): a click picks a word, `F12` and Ctrl+click go to its definition, and
     `Alt+F12` opens the Definition peek under its line, or below the code when `[diff] peek` says split
     (#459). The header says what Go to definition is doing, as the line over the diff does. -->
<script lang="ts">
  import { fileViewWords, type FileView } from "$lib/state/file-view.svelte";
  import { peekPlaces, type Peek } from "$lib/state/peek";
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import PeekBox from "./PeekBox.svelte";
  import PeekCode from "./PeekCode.svelte";
  import SplitPeek from "./SplitPeek.svelte";

  let { view, files }: { view: FileView; files: PrFilesState } = $props();

  const words = $derived(fileViewWords(view, files.number));
  /** The Definition peek, while it shows under a line of this view, inside its code. */
  const inline = $derived(files.peekOn(view, "inline"));
  /** The Definition peek, while it shows split below this view's code (#459). */
  const split = $derived(files.peekOn(view, "split"));
  /** Hears that the code took the peek's opening. */
  const revealed = () => {
    files.peekOpened = false;
  };
  /** Hands the focus to the code, so the keys scroll it. */
  const focusCode = (section: HTMLElement) => {
    section.querySelector<HTMLElement>("[data-scrolls]")?.focus({ preventScroll: true });
  };
</script>

{#snippet box(top: number, shown: Peek, rows: number)}
  <PeekBox diff={files} peek={shown} place={{ kind: "inline", top, rows, gutter: 48 }} />
{/snippet}

<section
  class="file-view"
  aria-label="File view"
  {@attach focusCode}
  {@attach (section) => {
    // The peek closed, or Go to definition from it landed on another line of this file.
    if (!files.viewWantsFocus) return;
    files.viewWantsFocus = false;
    focusCode(section);
  }}
>
  <div class="header">
    <button
      type="button"
      class="back"
      onclick={() => {
        files.closeFileView();
      }}>{words.back}</button
    >
    <span class="path">{view.place.path}</span>
    <span class="muted">{words.commit}</span>
    <span class="badge">Read-only</span>
    {#if files.notice !== undefined}
      <span class="notice" role="status">{files.notice}</span>
    {/if}
  </div>
  <SplitPeek {files} peek={split}>
    <div class="code">
      <PeekCode
        code={view.code}
        mark={view.landing}
        ongo={() => void files.goToDefinition()}
        inset={inline && {
          peek: inline,
          line: inline.at.line,
          rows: peekPlaces(inline, files.panes.peekRows),
          reveal: files.peekOpened,
          onreveal: revealed,
          box,
        }}
        sight={split && { line: split.at.line, reveal: files.peekOpened, onreveal: revealed }}
      />
    </div>
  </SplitPeek>
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

  /* What Go to definition is doing, or that it found nothing, as over the diff. */
  .notice {
    overflow: hidden;
    color: var(--fg-muted);
    text-overflow: ellipsis;
  }

  .code {
    flex: 1;
    min-height: 0;
  }
</style>
