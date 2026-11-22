<!-- A Repository's page in Settings (docs/devel/settings.md, #534). Its header names the Repository and says
     whether Tsugiki can write there, with why not, its open Issues and PRs as GitHub last counted them, and
     its last sync, which moves on while the page shows. Each part waits for the core's answer, and the line
     shows once one has come. -->
<script lang="ts">
  import { RepositoryStateView } from "$lib/state/repository-state.svelte";
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
  const sync = $derived(syncView?.state);
  const parts = $derived(
    [
      writeWords(stateView?.state, repository),
      sync?.open ? openWords(sync.open) : null,
      sync === undefined ? null : syncedWords(sync.last_sync, now),
    ].filter((part) => part !== null),
  );
</script>

<h3 class="mono">{repository}</h3>
{#if parts.length > 0}
  <p class="facts">{parts.join(" · ")}</p>
{/if}
<p class="empty">Nothing to set here yet.</p>

<style>
  .facts {
    margin: 0 0 12px;
    color: var(--fg-muted);
  }

  .empty {
    color: var(--fg-muted);
  }
</style>
