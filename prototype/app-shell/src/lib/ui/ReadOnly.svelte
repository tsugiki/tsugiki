<script lang="ts">
  // PROTOTYPE — throwaway. Under a composer when gh-rev can't write here: why, and the way out —
  // copy what was written and finish on GitHub, or fix the token.
  import { whyReadOnly } from "$lib/state/account.svelte";
  import { openSettings } from "$lib/state/settings.svelte";
  import { say } from "$lib/state/app.svelte";
  import { openInBrowser } from "$lib/platform/shell";
  let { text, url }: { text: string; url: string } = $props();
  const why = $derived(whyReadOnly());
  async function copyOpen() {
    try { await navigator.clipboard.writeText(text); say("Copied. Paste it on GitHub."); } catch { say("Couldn't copy: select the text and copy it by hand"); }
    openInBrowser(url);
  }
</script>

<div class="ro">
  <span>🔒 Read-only: {why}.</span>
  <button onclick={copyOpen} disabled={!text.trim()} title="Copies what you wrote and opens this on GitHub">Copy and open on GitHub</button>
  {#if why !== "Offline"}<button class="link" onclick={() => openSettings("Account")}>Token settings…</button>{/if}
</div>

<style>
  .ro { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; font: 12px var(--sans); color: var(--muted); margin-top: 6px; }
  .link { all: unset; cursor: pointer; color: var(--accent); }
</style>
