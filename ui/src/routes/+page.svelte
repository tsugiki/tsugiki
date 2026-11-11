<script lang="ts">
  import { AboutState } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import { applyTheme, ThemeState } from "$lib/state/theme.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { ChordWait } from "$lib/ui/chord.svelte";
  import { currentOs, keyGuard, keysForBrowser } from "$lib/ui/keys";
  import { mirrorNote } from "$lib/ui/mirror-note";
  import MirrorQuestionDialog from "$lib/ui/MirrorQuestionDialog.svelte";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import Palette, { type PaletteCommand, type PaletteWords } from "$lib/ui/Palette.svelte";
  import PerformanceHud from "$lib/ui/PerformanceHud.svelte";
  import RepositoryHome from "$lib/ui/RepositoryHome.svelte";
  import StatusBar from "$lib/ui/StatusBar.svelte";

  const PICKER_WORDS: PaletteWords = {
    dialog: "Repository picker",
    input: "Repository",
    placeholder: "Type a Repository's name",
    list: "Repositories",
    none: "No Repository matches",
  };

  const theme = new ThemeState(matchMedia("(prefers-color-scheme: dark)"));
  void theme.load();
  $effect(() => {
    if (theme.current) applyTheme(document.documentElement, theme.current);
  });
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
  let pickerOpen = $state(false);
  /** The Repository whose mirror's question the dialog asks, while it's open. */
  let questionFor = $state<string>();
  /** Opens the Repository picker, which only a Repository's screen has. */
  const openPicker = () => {
    // Each dialog closes on Esc, so one on top of another would lose what the user typed below.
    if (home.current?.screen === "repository" && !addOpen && !paletteOpen && questionFor === undefined)
      pickerOpen = true;
  };
  /** Whether the list's keys and the chords work: a dialog, the palette or the picker on top keeps them. */
  const keysActive = $derived(!addOpen && !paletteOpen && !pickerOpen && questionFor === undefined);
  const os = currentOs();
  const chord = new ChordWait(os);
  const guard = keyGuard(
    {
      "ctrl+shift+p": () => {
        paletteOpen = !addOpen && !pickerOpen && questionFor === undefined;
      },
      "ctrl+r": openPicker,
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
  /** The name of the open Repository, if one is. */
  const openName = $derived(home.current?.screen === "repository" ? home.current.repository.name : undefined);
  /** The Repositories other than the open one, most recently opened first. */
  const others = $derived(home.current?.screen === "repository" ? home.current.others : []);
  /** Switches to `name`. The core read the files again to switch, so a problem the user has since fixed
   * can go. */
  const switchTo = (name: string) => {
    void home.open(name).then(() => settings.load());
  };
  /** A row that switches to each Repository but the open one, titled `prefix` and its name, with the note
   * `noteOf` gives it. */
  const switchRows = (
    prefix: string,
    noteOf: (name: string) => string | null = () => null,
  ): PaletteCommand[] =>
    others.map((name) => ({
      title: `${prefix}${name}`,
      note: noteOf(name),
      run: () => {
        switchTo(name);
      },
    }));
  /** Why `name`'s diffs aren't there yet, while its mirror isn't ready. */
  const mirrorNoteOf = (name: string) =>
    mirrorNote(name, status.mirrorStates.find((mirror) => mirror.repository === name)?.state);
  /** The picker's rows: the open Repository, named `open`, the others, then Add repository…. It opens on
   * the one opened before the open one, so Ctrl+R and Enter go back to it. */
  const pickerRows = (open: string): PaletteCommand[] => [
    { title: open, current: true, note: mirrorNoteOf(open), run: () => {} },
    ...switchRows("", mirrorNoteOf),
    { title: "Add repository…", run: openAdd },
  ];
  /** The open Repository's last sync. After a switch the core tells of the new one's a moment later, and
   * until then the status bar shows no last sync. */
  const lastSync = $derived(
    openName === undefined || status.lastSync?.repository === openName ? status.lastSync : null,
  );
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
    ...switchRows("Repository: "),
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
    {#if home.switchProblem}
      <p role="alert">{home.switchProblem}</p>
    {/if}
    <RepositoryHome
      bind:this={repositoryHome}
      name={home.current.repository.name}
      onadd={openAdd}
      onpick={openPicker}
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
  {lastSync}
  syncActivity={status.syncActivity}
  cloning={status.cloning}
  mirrorProblem={status.mirrorProblem}
  mirrorQuestion={status.mirrorQuestion}
  newVersion={status.newVersion}
  chord={chord.first}
  onrefresh={() => void status.refresh()}
  oncheckagain={() => void status.checkMirrorsAgain()}
  onquestion={(repository) => {
    questionFor = repository;
  }}
/>
{#if questionFor !== undefined}
  <MirrorQuestionDialog
    repository={questionFor}
    onclose={() => {
      questionFor = undefined;
    }}
  />
{/if}
{#if addOpen}
  <AddRepository
    onclose={() => {
      addOpen = false;
    }}
    onadded={(next) => {
      home.added(next);
      addOpen = false;
      // The core read the files again to add this, so a problem the user has since fixed can go.
      void settings.load();
    }}
    onswitch={(name) => {
      addOpen = false;
      if (name !== openName) switchTo(name);
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
{#if pickerOpen && openName !== undefined}
  <Palette
    commands={pickerRows(openName)}
    words={PICKER_WORDS}
    start={others.length > 0 ? 1 : 0}
    onclose={() => {
      pickerOpen = false;
    }}
  />
{/if}
{#if hud.visible}
  <PerformanceHud {hud} refreshing={status.syncActivity !== null} />
{/if}
