<script lang="ts">
  import type { TokenSource } from "$lib/state/home.svelte";

  let { token, onadd }: { token: TokenSource; onadd: () => void } = $props();

  const WHERE_FROM: Record<TokenSource, string> = {
    gh: "Your token comes from the GitHub CLI (gh auth token).",
  };
</script>

<section>
  <p>{WHERE_FROM[token]}</p>
  <p>Add a Repository to start.</p>
  <button onclick={onadd}>Add repository</button>
</section>
