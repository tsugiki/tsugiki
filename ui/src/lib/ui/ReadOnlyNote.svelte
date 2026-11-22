<!-- The line atop the Issue and PR views of a Read-only Repository, which says why Tsugiki can't write there
     (#398). Nothing, while it can. -->
<script lang="ts">
  import { type RepositoryState, readOnlyWords } from "$lib/state/repository-state.svelte";

  let { repositoryState, repository }: { repositoryState: RepositoryState | undefined; repository: string } =
    $props();

  const words = $derived(readOnlyWords(repositoryState, repository));
</script>

{#if words !== null}
  <p class="read-only">{words}</p>
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
