<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import { currentOs, keyGuard, keysForBrowser } from "$lib/ui/keys";
  import Palette from "$lib/ui/Palette.svelte";
  import StatusBar from "$lib/ui/StatusBar.svelte";

  const about = new AboutState();
  void about.load();
  const status = new StatusState();
  void status.load();
  const settings = new SettingsState();
  void settings.load();

  let paletteOpen = $state(false);
  const guard = keyGuard(
    {
      "ctrl+shift+p": () => {
        paletteOpen = true;
      },
    },
    currentOs(),
    keysForBrowser(import.meta.env.DEV),
  );
</script>

<svelte:window onkeydowncapture={guard} />

<main>
  <AboutLine about={about.about} error={about.error} />
</main>
<StatusBar
  status={status.status}
  error={status.error}
  visible={settings.statusBarVisible}
  problems={settings.problems}
/>
{#if paletteOpen}
  <Palette
    onclose={() => {
      paletteOpen = false;
    }}
  />
{/if}
