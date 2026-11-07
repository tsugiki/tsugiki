<!-- A PR's Files tab: its diff, or what it waits for, the mirror's clone or the PR's commits. -->
<script lang="ts">
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import DiffRows from "./DiffRows.svelte";
  import { cloningWords } from "./synced";

  let { files, now, onlink }: { files: PrFilesState; now: Date; onlink: (address: string) => void } =
    $props();
</script>

{#if files.error}
  <p role="alert">{`Tsugiki can't show this PR's files: ${files.error}`}</p>
{/if}
{#if files.view?.state === "cloning"}
  <p class="waiting">{cloningWords(files.cloning) ?? "Cloning the Repository's mirror…"}</p>
{:else if files.view?.state === "fetching"}
  <p class="waiting">Fetching the PR's commits…</p>
{:else if files.view?.state === "ready"}
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
