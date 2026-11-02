<script lang="ts">
  import type { About } from "$lib/state/about.svelte";

  let { about, error }: { about: About | undefined; error?: string | undefined } = $props();
</script>

{#if about}
  <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
  <p>{`${about.name} ${about.version}`}</p>
{:else if error}
  <p>The core didn't answer: {error}</p>
{:else}
  <p>Asking the core…</p>
{/if}
