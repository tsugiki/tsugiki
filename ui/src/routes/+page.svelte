<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { currentOs, keyGuard, keysForBrowser } from "$lib/ui/keys";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import Palette from "$lib/ui/Palette.svelte";
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
    },
    currentOs(),
    keysForBrowser(import.meta.env.DEV),
  );
</script>

<svelte:window onkeydowncapture={guard} />

<main>
  <AboutLine about={about.about} error={about.error} />
  {#if home.current?.screen === "onboarding"}
    <Onboarding token={home.current.token} onadd={openAdd} />
  {:else if home.current?.screen === "repository"}
    <RepositoryHome name={home.current.repository.name} onadd={openAdd} />
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
    onclose={() => {
      paletteOpen = false;
    }}
  />
{/if}
