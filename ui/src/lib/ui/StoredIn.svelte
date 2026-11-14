<!-- Where a setting is stored, beside it on its Settings page (ADR 0014): the settings file, which can travel
     between machines, this machine's file, the database, or the OS keychain. -->
<script lang="ts">
  import type { StoredIn } from "$lib/state/settings-dialog.svelte";

  let { where }: { where: StoredIn } = $props();

  const WORDS: Readonly<Record<StoredIn, string>> = {
    settings: "settings file",
    machine: "this machine",
    database: "database",
    keychain: "keychain",
  };
  const TIPS: Readonly<Record<StoredIn, string>> = {
    settings: "Saved in settings.toml, which can travel between machines",
    machine: "Saved in machine.toml, on this machine only",
    database: "Saved in Tsugiki's database, on this machine only",
    keychain: "Saved in the OS keychain, on this machine only",
  };
</script>

<span class={["stored", where]} title={TIPS[where]}>{WORDS[where]}</span>

<style>
  .stored {
    padding: 0 6px;
    font-size: 10.5px;
    white-space: nowrap;
    color: var(--fg-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
  }

  .settings {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
</style>
