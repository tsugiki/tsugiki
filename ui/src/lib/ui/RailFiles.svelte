<!-- The rail's Files tab (#604): the PR's changed files as a tree of folders, under a box that filters them
     by path. Each file's row shows its name, its +/− counts, and how many review threads and drafts it
     holds, and a Generated file's row is greyed, as the Files tab collapses it. ↓ in the box goes to the
     tree, where ← → fold and unfold a folder, and Enter or a click opens a file in the Files tab. The file
     on screen there is marked, and the tree scrolls to keep it in view. The PR's diff is read once the tab
     shows, on either of the PR's tabs. -->
<script lang="ts">
  import { untrack } from "svelte";
  import type { ShownRow } from "$lib/state/file-tree.svelte";
  import type { FileChange } from "$lib/state/pr-files.svelte";
  import type { PrViewState } from "$lib/state/pr-view.svelte";
  import { currentOs, keyName } from "./keys";
  import RailList, { type RailRow } from "./RailList.svelte";

  let { pr }: { pr: PrViewState } = $props();

  const tree = $derived(pr.tree);
  const rows = $derived(tree.rows);
  let list = $state<RailList>();

  // It runs again for another PR, and for nothing the call itself reads.
  $effect(() => {
    const view = pr;
    untrack(() => {
      view.loadFiles();
    });
  });

  /** The box's ↓, which goes to the tree. */
  function onboxkeydown(e: KeyboardEvent) {
    if (keyName(e, currentOs()) !== "arrowdown" || rows.length === 0) return;
    e.preventDefault();
    e.stopPropagation();
    list?.focus();
  }

  /** Opens the file under the cursor in the Files tab, or flips the fold of the folder under it. */
  function pick() {
    const file = tree.pick();
    if (file !== undefined) pr.goToFile(file);
  }

  /** How row `index` shows: a folder open or folded, a Generated file greyed, the file on screen marked,
   * and the path in its tooltip, with where a renamed file was. */
  function shows(index: number): RailRow {
    const row = rows[index] as ShownRow;
    const shown = { level: row.row.depth + 1, current: index === tree.current };
    if (row.kind === "folder")
      return { ...shown, expanded: !row.folded, title: row.row.path, class: "folder" };
    const { path, old_path, generated } = row.file;
    const title = old_path === null ? path : `${old_path} → ${path}`;
    return { ...shown, title, class: ["file", generated && "generated"] };
  }

  /** A file's counts of added and removed lines, or that git counts none. */
  const counts = (change: FileChange) =>
    change.kind === "binary" ? ["binary"] : [`+${String(change.added)}`, `−${String(change.removed)}`];

  /** The words of a count of `n` things, each called `one`. */
  const many = (n: number, one: string) => `${String(n)} ${one}${n === 1 ? "" : "s"}`;
</script>

<!-- A row's pieces, with no space between them: the row's gap spaces them, and its text reads as one. -->
{#snippet drawn(index: number)}
  {@const row = rows[index] as ShownRow}
  <!-- prettier-ignore -->
  {#if row.kind === "folder"}<span class="chevron" aria-hidden="true">{row.folded ? "▸" : "▾"}</span><span class="name">{row.row.name}</span>{:else}<span class="chevron" aria-hidden="true"></span><span class="name">{row.row.name}</span>{#if row.file.generated}<span class="tag">Generated</span>{/if}{#if row.row.threads > 0}<span class="note-count" title={many(row.row.threads, "review thread")}>{row.row.threads}</span>{/if}{#if row.row.drafts > 0}<span class="note-count draft" title={many(row.row.drafts, "draft")}>{row.row.drafts}</span>{/if}<span class="counts">{#each counts(row.file.change) as count (count)}<span>{count}</span>{/each}</span>{/if}
{/snippet}

<div class="files">
  <div class="rail-box">
    <input
      type="text"
      aria-label="Filter the changed files"
      placeholder="Filter files"
      spellcheck="false"
      value={tree.filter}
      oninput={(e) => {
        tree.type(e.currentTarget.value);
      }}
      onkeydown={onboxkeydown}
    />
  </div>
  {#if tree.unready !== undefined}
    <p class="note" aria-live="polite">{tree.unready}</p>
  {:else if rows.length === 0 && tree.filter !== ""}
    <p class="note" aria-live="polite">{`No file's path holds “${tree.filter}”.`}</p>
  {/if}
  {#if rows.length > 0}
    <RailList
      bind:this={list}
      label="Changed files"
      count={rows.length}
      cursor={tree.cursor}
      indent={12}
      {shows}
      row={drawn}
      reveal={tree.current}
      onmove={(index) => {
        tree.moveTo(index);
      }}
      onfold={() => {
        tree.fold();
      }}
      onunfold={() => {
        tree.unfold();
      }}
      onpick={pick}
    />
  {/if}
</div>

<style>
  /* Fills the rail's pane: the box stays put, and the tree scrolls under it. */
  .files {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .rail-box {
    margin-bottom: 4px;
  }

  .note {
    margin: 4px 8px;
    font-size: 12px;
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }

  .chevron {
    flex: none;
    width: 12px;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .tag,
  .counts,
  .note-count {
    flex: none;
    font-size: 11px;
    color: var(--fg-muted);
  }

  .counts {
    display: flex;
    gap: 4px;
  }

  .note-count {
    padding: 0 6px;
    border-radius: 8px;
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  /* The theme's attention colour, for the drafts only the user sees, as a draft in the Files tab has. */
  .note-count.draft {
    color: var(--attention);
    border: 1px solid var(--attention);
    background: none;
  }
</style>
