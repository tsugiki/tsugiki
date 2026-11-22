<!-- A Repository's page in Settings (docs/devel/settings.md, #534). Its header names the Repository and says
     whether Tsugiki can write there, its open Issues and PRs as GitHub last counted them, and its last sync,
     which moves on while the page shows. Each part waits for the core's answer. -->
<script lang="ts">
  import { readOnlyWords, RepositoryStateView } from "$lib/state/repository-state.svelte";
  import { RepositorySyncView } from "$lib/state/repository-sync.svelte";
  import { minuteClock } from "./clock.svelte";
  import { openWords, writeWords } from "./repository-header";
  import { syncedWords } from "./synced";

  let { repository }: { repository: string } = $props();

  // Another Repository's page asks about that one, so a switch starts new views and stops the last.
  let stateView = $state<RepositoryStateView>();
  let syncView = $state<RepositorySyncView>();
  $effect(() => {
    const views = { state: new RepositoryStateView(repository), sync: new RepositorySyncView(repository) };
    stateView = views.state;
    syncView = views.sync;
    void views.state.load();
    void views.sync.load();
    return () => {
      views.state.stop();
      views.sync.stop();
    };
  });

  // "Synced 2 min ago" has to move on by itself between syncs.
  const now = minuteClock();
  const repositoryState = $derived(stateView?.state);
  const writes = $derived(writeWords(repositoryState));
  const sync = $derived(syncView?.state);
</script>

<h3 class="mono">{repository}</h3>
<p class="facts">
  {#if writes !== null}
    <span title={readOnlyWords(repositoryState, repository) ?? undefined}>{writes}</span>
  {/if}
  {#if sync?.open}
    <span>{openWords(sync.open)}</span>
  {/if}
  {#if sync !== undefined}
    <span>{syncedWords(sync.last_sync, now)}</span>
  {/if}
</p>
<p class="empty">Nothing to set here yet.</p>

<style>
  .facts {
    margin: 0 0 12px;
    color: var(--fg-muted);
  }

  .facts span + span::before {
    content: " · ";
  }

  .empty {
    color: var(--fg-muted);
  }
</style>
