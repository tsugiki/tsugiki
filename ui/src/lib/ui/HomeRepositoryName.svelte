<script lang="ts">
  import { RepositoryStateView } from "$lib/state/repository-state.svelte";

  let { name }: { name: string } = $props();

  let view = $state<RepositoryStateView>();
  $effect(() => {
    const state = new RepositoryStateView(name);
    view = state;
    void state.load();
    return () => {
      state.stop();
    };
  });
</script>

<span class="name mono">{name}</span>
{#if (view?.state?.read_only.length ?? 0) > 0}
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
