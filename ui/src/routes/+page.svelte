<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { ChordWait } from "$lib/ui/chord.svelte";
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
  // WKWebView has no PerformanceObserver for keys and long frames, and the miss log asks before it listens.
  hud.watchBrowser(globalThis.PerformanceObserver);

  let addOpen = $state(false);
  const openAdd = () => {
    addOpen = true;
  };
  let paletteOpen = $state(false);
  /** Whether the list's keys and the chords work: a dialog or the palette on top keeps them. */
  const keysActive = $derived(!addOpen && !paletteOpen);
  const os = currentOs();
  const chord = new ChordWait(os);
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
    os,
    keysForBrowser(import.meta.env.DEV),
  );
  /** Reads each key before anything else, so the key that ends a chord's wait goes nowhere else. Chords
   * start where the list's keys work: on a Repository's screen, with no dialog or palette on top, and the
   * screen says what each does. */
  const readKey = (e: KeyboardEvent) => {
    const canStart = home.current?.screen === "repository" && keysActive;
    if (!chord.read(e, canStart, repositoryHome?.chordBindings() ?? {})) guard(e);
  };
  const endChord = () => {
    chord.end();
  };
  /** Names the open Repository, so the user can tell which one this is with the Sidebar hidden, in the
   * taskbar and in Alt+Tab. `startup.rs` copies it to the window. */
  const title = $derived(
    home.current?.screen === "repository" ? `${home.current.repository.name} · Tsugiki` : "Tsugiki",
  );
  // A run that opens on Onboarding has no Workbench to time until the user adds a Repository.
  $effect(() => {
    if (home.current?.screen === "onboarding" || home.error) hud.skipStartup();
  });
  /** The open Repository's screen, which adds its own commands to the palette. */
  let repositoryHome = $state<ReturnType<typeof RepositoryHome>>();
  const commands: PaletteCommand[] = $derived([
    ...(repositoryHome?.paletteCommands() ?? []),
    {
      title: hud.visible ? "Hide performance HUD" : "Show performance HUD",
      run: () => {
        hud.toggle();
      },
    },
  ]);
</script>

<svelte:window onkeydowncapture={readKey} onpointerdowncapture={endChord} onblur={endChord} />
<!-- A template string: Svelte compiles a bare `{title}` with a `?? ""` branch that no test can reach. -->
<svelte:head><title>{`${title}`}</title></svelte:head>

<main>
  <AboutLine about={about.about} error={about.error} />
  {#if home.current?.screen === "onboarding"}
    <Onboarding token={home.current.token} onadd={openAdd} />
  {:else if home.current?.screen === "repository"}
    <RepositoryHome
      bind:this={repositoryHome}
      name={home.current.repository.name}
      onadd={openAdd}
      {keysActive}
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
  cloning={status.cloning}
  mirrorProblem={status.mirrorProblem}
  mirrorQuestion={status.mirrorQuestion}
  chord={chord.first}
  onrefresh={() => void status.refresh()}
  oncheckagain={() => void status.checkMirrorsAgain()}
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
