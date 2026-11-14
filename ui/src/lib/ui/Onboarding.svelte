<script lang="ts">
  import type { TokenSource } from "$lib/state/home.svelte";
  import { currentOs, keyLabel } from "./keys";
  import { TOKEN_SOURCE_WORDS } from "./token-words";

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
</script>

<section>
  <button
    class="settings"
    aria-haspopup="dialog"
    aria-label="Settings"
    title={`Settings (${keyLabel("ctrl+,", currentOs())})`}
    onclick={onsettings}>⚙ Settings</button
  >
  <p>{TOKEN_SOURCE_WORDS[token]}</p>
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
