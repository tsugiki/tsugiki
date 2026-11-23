<!-- The line atop the Issue and PR views of a Read-only Repository, which says why Tsugiki can't write there
     (#398). The user's choice links to the Repository's page in Settings, which switches it (#915). Nothing,
     while it can. -->
<script lang="ts">
  import { type RepositoryState, readOnlyPieces } from "$lib/state/repository-state.svelte";
  import { settingsAsk } from "$lib/state/settings-ask.svelte";

  let { repositoryState, repository }: { repositoryState: RepositoryState | undefined; repository: string } =
    $props();

  const pieces = $derived(readOnlyPieces(repositoryState, repository));
</script>

{#if pieces !== null}
  <p class="read-only">
    {#each pieces as piece, at (at)}{#if piece.settings}<button
          class="link"
          onclick={() => {
            settingsAsk.openRepository(repository);
          }}>{piece.text}</button
        >{:else}{piece.text}{/if}{/each}
  </p>
{/if}

<style>
  .read-only {
    margin: 0 0 8px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    background: var(--bg-chrome);
    color: var(--fg-chrome);
  }
</style>
