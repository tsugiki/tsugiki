<script lang="ts">
  import Row from "./Row.svelte";

  let { rows }: { rows: string[] } = $props();
  let count = $derived(rows.length);
</script>

{#if count > 0}
  <ul class="rows">
    {#each rows as row, index (index)}
      <Row text={row} on:click={() => console.log(index)} />
    {/each}
  </ul>
{:else}
  <p>No rows</p>
{/if}

<style>
  .rows { margin: 0; }
</style>
