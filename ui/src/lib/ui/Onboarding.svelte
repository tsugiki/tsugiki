<script lang="ts">
  import type { TokenSource } from "$lib/state/home.svelte";

  let {
    token,
    onadd,
    onsettings,
  }: {
    token: TokenSource;
    onadd: () => void;
    /** Opens the Settings dialog. */
    onsettings: () => void;
  } = $props();

  const WHERE_FROM: Record<TokenSource, string> = {
    gh: "Your token comes from the GitHub CLI (gh auth token).",
  };
</script>

<section>
  <button
    class="settings"
    aria-haspopup="dialog"
    aria-label="Settings"
    title="Settings (Ctrl+,)"
    onclick={onsettings}>⚙ Settings</button
  >
  <p>{WHERE_FROM[token]}</p>
  <p>Add a Repository to start.</p>
  <button onclick={onadd}>Add repository</button>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }

  /* At the top right, as on the prototype's Home page. */
  .settings {
    align-self: flex-end;
  }
</style>
