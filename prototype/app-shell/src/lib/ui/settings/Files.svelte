<script lang="ts">
  // PROTOTYPE — throwaway. What the settings above would write, live: the synced config.toml and this
  // machine's local.toml. It is how the prototype surfaces its state.
  import { configText, localText, value } from "$lib/state/settings.svelte";
  let tab = $state<"config" | "local">("config");
  const text = $derived(tab === "config" ? configText() : localText());
</script>

<div class="files">
  <div class="tabs">
    <button class:on={tab === "config"} onclick={() => (tab = "config")} title={String(value("storage.config_file"))}>config.toml <span class="muted">synced</span></button>
    <button class:on={tab === "local"} onclick={() => (tab = "local")}>local.toml <span class="muted">this machine</span></button>
  </div>
  <pre>{text}</pre>
</div>

<style>
  .files { display: flex; flex-direction: column; min-height: 0; height: 100%; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--line); }
  .tabs button { all: unset; cursor: pointer; padding: 5px 10px; font-size: 12px; border-bottom: 2px solid transparent; }
  .tabs button.on { border-bottom-color: var(--accent); font-weight: 600; }
  .muted { color: var(--muted); font-weight: 400; }
  pre { flex: 1; margin: 0; overflow: auto; padding: 8px 10px; font: 12px/1.45 var(--mono); background: var(--panel); }
</style>
