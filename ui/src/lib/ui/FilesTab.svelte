<!-- A PR's Files tab: its diff, or what it waits for, the mirror's clone or the PR's commits. When the PR
     has moved on since the mirror's copy, the diff before shows while the mirror fetches the newer one, with
     its threads held back and counted (#185). Ctrl+F opens a search over the diff's rows (#186). -->
<script lang="ts">
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import DiffRows from "./DiffRows.svelte";
  import FindBar from "./FindBar.svelte";
  import { cloningWords } from "./synced";

  let { files, now, onlink }: { files: PrFilesState; now: Date; onlink: (address: string) => void } =
    $props();

  /** A commit's short name, as GitHub shows it. */
  const short = (commit: string) => commit.slice(0, 7);
  const waiting = (count: number) =>
    count === 1 ? "1 thread waits for it." : `${String(count)} threads wait for it.`;
</script>

{#if files.error}
  <p role="alert">{`Tsugiki can't show this PR's files: ${files.error}`}</p>
{/if}
{#if files.view?.state === "cloning"}
  <p class="waiting">{cloningWords(files.cloning) ?? "Cloning the Repository's mirror…"}</p>
{:else if files.view?.state === "fetching"}
  <p class="waiting">Fetching the PR's commits…</p>
{:else if files.view?.state === "ready"}
  {#if files.heldBack !== undefined}
    <p class="waiting" role="status">
      {`Showing ${short(files.view.head)} · fetching the newer ${short(files.heldBack.newer)}.`}
      {#if files.heldBack.count > 0}{waiting(files.heldBack.count)}{/if}
    </p>
  {/if}
  {#if files.searching}
    <FindBar {files} />
  {/if}
  <div class="diff">
    <DiffRows diff={files} files={files.view.files} news={files.view.news} {now} {onlink} />
  </div>
{/if}

<style>
  .waiting {
    font-size: 12px;
    opacity: 0.75;
  }

  /* The diff fills what the tab leaves, and scrolls by itself. */
  .diff {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
