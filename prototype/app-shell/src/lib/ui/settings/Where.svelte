<script lang="ts">
  // PROTOTYPE — throwaway. Which file a setting goes to: the synced config.toml, this machine's
  // local.toml, or the OS keychain.
  import type { Where } from "$lib/core/settings";
  let { where }: { where: Where } = $props();
  const text = { synced: "synced", machine: "this machine", keychain: "keychain" };
  const tip = {
    synced: "Saved in config.toml, which you sync between machines",
    machine: "Saved in local.toml, on this machine only",
    keychain: "Saved in the OS keychain, on this machine only",
  };
</script>

<span class="w {where}" title={tip[where]}>{text[where]}</span>

<style>
  .w { font-size: 10.5px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--line); color: var(--muted); white-space: nowrap; }
  .synced { color: var(--accent); border-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .keychain { color: var(--warn); border-color: color-mix(in srgb, var(--warn) 40%, transparent); }
</style>
