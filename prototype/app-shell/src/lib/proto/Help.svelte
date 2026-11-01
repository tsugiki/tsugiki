<script lang="ts">
  // PROTOTYPE — throwaway. The `?` overlay.
  import { KEY_HELP } from "./keys";
  let { open = $bindable(false), extra = [] }: { open: boolean; extra?: [string, string][] } = $props();
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (open = false)}>
    <div class="help">
      <h3>Keys</h3>
      <table><tbody>
        {#each [...extra, ...KEY_HELP] as [k, d]}<tr><td><kbd>{k}</kbd></td><td>{d}</td></tr>{/each}
      </tbody></table>
      <p class="muted">On macOS, Cmd replaces Ctrl. Click anywhere to close.</p>
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.2); z-index: 60; display: grid; place-items: center; }
  .help { background: var(--bg); border: 1px solid var(--line); border-radius: 8px; padding: 12px 20px; font: 13px var(--sans); max-height: 80vh; overflow: auto; }
  h3 { margin: 4px 0 8px; }
  td { padding: 2px 12px 2px 0; }
  kbd { font: 12px var(--mono); border: 1px solid var(--line); border-radius: 3px; padding: 0 4px; background: var(--panel); }
  .muted { color: var(--muted); }
</style>
