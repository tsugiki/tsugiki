<script lang="ts">
  import { type RepositoryState, RepositoryStateView } from "$lib/state/repository-state.svelte";

  let {
    name,
    openState,
  }: {
    name: string;
    openState: (() => RepositoryState | undefined) | undefined;
  } = $props();

  let view = $state<RepositoryStateView>();
  $effect(() => {
    if (openState) return;
    const state = new RepositoryStateView(name);
    view = state;
    void state.load();
    return () => {
      state.stop();
    };
  });
  const latest = $derived(openState ? openState() : view?.state);
</script>

<span class="name mono">{name}</span>
{#if (latest?.read_only.length ?? 0) > 0}
  <span class="read-only">Read-only</span>
{/if}

<style>
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .read-only {
    font-size: 12px;
    color: var(--fg-muted);
    white-space: nowrap;
  }
</style>
