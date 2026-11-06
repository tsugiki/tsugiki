<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { currentOs, keyGuard, keysForBrowser } from "$lib/ui/keys";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import Palette, { type PaletteCommand } from "$lib/ui/Palette.svelte";
  import PerformanceHud from "$lib/ui/PerformanceHud.svelte";
  import RepositoryHome from "$lib/ui/RepositoryHome.svelte";
  import StatusBar from "$lib/ui/StatusBar.svelte";

  const about = new AboutState();
  void about.load();
  const status = new StatusState();
  void status.load();
  const settings = new SettingsState();
  void settings.load();
  const home = new HomeState();
  void home.load();

  let addOpen = $state(false);
  const openAdd = () => {
    addOpen = true;
  };
  let paletteOpen = $state(false);
  const guard = keyGuard(
    {
      "ctrl+shift+p": () => {
        // Both dialogs close on Esc, so one on top of the other would lose what the user typed below.
        paletteOpen = !addOpen;
      },
      "ctrl+shift+h": () => {
        hud.toggle();
      },
    },
    currentOs(),
    keysForBrowser(import.meta.env.DEV),
  );
  // A run that opens on Onboarding has no Workbench to time until the user adds a Repository.
  $effect(() => {
    if (home.current?.screen === "onboarding" || home.error) hud.skipStartup();
  });
  const commands: PaletteCommand[] = $derived([
    {
      title: hud.visible ? "Hide performance HUD" : "Show performance HUD",
      run: () => {
        hud.toggle();
      },
    },
  ]);
</script>

<svelte:window onkeydowncapture={guard} />

<main>
  <AboutLine about={about.about} error={about.error} />
  {#if home.current?.screen === "onboarding"}
    <Onboarding token={home.current.token} onadd={openAdd} />
  {:else if home.current?.screen === "repository"}
    <RepositoryHome
      name={home.current.repository.name}
      onadd={openAdd}
      keysActive={!addOpen && !paletteOpen}
    />
  {:else if home.error}
    <!-- One expression: Svelte compiles `{a} {b}` with `?? ""` branches that no test can reach. -->
    <p>{`Tsugiki can't say what to open: ${home.error}`}</p>
  {/if}
</main>
<StatusBar
  status={status.status}
  error={status.error}
  visible={settings.statusBarVisible}
  problems={settings.problems}
  lastSync={status.lastSync}
  syncActivity={status.syncActivity}
  onrefresh={() => void status.refresh()}
/>
{#if addOpen}
  <AddRepository
    onclose={() => {
      addOpen = false;
    }}
    onadded={(next) => {
      home.current = next;
      addOpen = false;
      // The core read the files again to add this, so a problem the user has since fixed can go.
      void settings.load();
    }}
  />
{/if}
{#if paletteOpen}
  <Palette
    {commands}
    onclose={() => {
      paletteOpen = false;
    }}
  />
{/if}
{#if hud.visible}
  <PerformanceHud {hud} refreshing={status.syncActivity !== null} />
{/if}
