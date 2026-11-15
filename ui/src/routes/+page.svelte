<script lang="ts">
  import { untrack } from "svelte";
  import { AboutState, openDevtools } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import { type Action, startsChordOf } from "$lib/state/keymap.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import { RecentCommands } from "$lib/state/recent-commands.svelte";
  import type { ServerName } from "$lib/state/server-words";
  import { settingsAsk } from "$lib/state/settings-ask.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import { applyTheme, ThemeState } from "$lib/state/theme.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { type ChordRun, ChordWait } from "$lib/ui/chord.svelte";
  import { type Bindings, currentOs, isTextBox, keyGuard, typesInTextBox } from "$lib/ui/keys";
  import { mirrorNote } from "$lib/ui/mirror-note";
  import MirrorQuestionDialog from "$lib/ui/MirrorQuestionDialog.svelte";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import Palette, { type PaletteCommand, type PaletteWords } from "$lib/ui/Palette.svelte";
  import PerformanceHud from "$lib/ui/PerformanceHud.svelte";
  import RepositoryHome from "$lib/ui/RepositoryHome.svelte";
  import Settings from "$lib/ui/Settings.svelte";
  import StatusBar from "$lib/ui/StatusBar.svelte";
  import ThemePicker from "$lib/ui/ThemePicker.svelte";
  import "$lib/ui/window.css";

  const PICKER_WORDS: PaletteWords = {
    dialog: "Repository picker",
    input: "Repository",
    placeholder: "Type a Repository's name",
    list: "Repositories",
    none: "No Repository matches",
  };

  const theme = new ThemeState(matchMedia("(prefers-color-scheme: dark)"));
  void theme.load();
  // The theme picker's preview too, with no fade.
  $effect(() => {
    if (theme.shown) applyTheme(document.documentElement, theme.shown);
  });
  const about = new AboutState();
  void about.load();
  const status = new StatusState();
  void status.load();
  const settings = new SettingsState();
  void settings.load();
  const home = new HomeState();
  void home.load();
  const recentCommands = new RecentCommands();
  void recentCommands.load();
  // WKWebView has no PerformanceObserver for keys and long frames, and the miss log asks before it listens.
  hud.watchBrowser(globalThis.PerformanceObserver);

  let addOpen = $state(false);
  let paletteOpen = $state(false);
  let pickerOpen = $state(false);
  let themePickerOpen = $state(false);
  let settingsOpen = $state(false);
  /** The Repository whose mirror's question the dialog asks, while it's open. */
  let questionFor = $state<string>();
  /** Whether a dialog, the palette or a picker is open. Each closes on Esc, so one on top of another would
   * lose what the user typed below, and none opens over another: a control behind one, which a press or Tab
   * can still reach, does nothing. A command run from the palette runs once it has closed. */
  const dialogOpen = $derived(
    addOpen || paletteOpen || pickerOpen || themePickerOpen || settingsOpen || questionFor !== undefined,
  );
  const openAdd = () => {
    if (!dialogOpen) addOpen = true;
  };
  /** Opens the Repository picker, which only a Repository's screen has. */
  const openPicker = () => {
    if (home.current?.screen === "repository" && !dialogOpen) pickerOpen = true;
  };
  /** The language server whose row in Settings › Advanced the dialog opens at, if it opened at one. */
  let settingsAt = $state<ServerName>();
  /** Opens the Settings dialog on its General page, from `Ctrl+,`, ⚙ or the palette. */
  const openSettings = () => {
    if (!dialogOpen) {
      settingsAt = undefined;
      settingsOpen = true;
    }
  };
  // The Definition peek's link asks for a server's row in Settings › Advanced (#683). With another dialog
  // open the ask goes, as a press behind a dialog does nothing.
  $effect(() => {
    if (settingsAsk.server === undefined) return;
    const server = settingsAsk.take();
    if (!untrack(() => dialogOpen)) {
      settingsAt = server;
      settingsOpen = true;
    }
  });
  /** Opens the theme picker, from `Ctrl+K T` or the palette. Neither reaches it with a dialog open: the key's
   * binding asks for none, and the palette has closed by then. It reads the themes folder again, so a file
   * saved since shows up as the list fills in. */
  const openThemePicker = () => {
    themePickerOpen = true;
    void theme.load();
  };
  /** Reads the themes folder again, from `Ctrl+K R` or the palette: a theme file edited since takes effect,
   * and the status bar names what is wrong with each (#519). */
  const reloadThemes = () => {
    void theme.load();
  };
  /** Whether the list's keys and the chords work: a dialog, the palette or a picker on top keeps them. */
  const keysActive = $derived(!dialogOpen);
  const os = currentOs();
  const chord = new ChordWait(os);
  /** The actions the page runs, on a key of one press or a chord, as `[keys]` may give either (#549). */
  const pageBindings: Bindings = {
    command_palette: () => {
      if (!dialogOpen) paletteOpen = true;
    },
    switch_repository: openPicker,
    open_settings: openSettings,
    toggle_hud: () => {
      hud.toggle();
    },
    pick_theme: () => {
      if (!dialogOpen) openThemePicker();
    },
    reload_themes: reloadThemes,
  };
  const guard = keyGuard(pageBindings, os);
  const pageActions = Object.keys(pageBindings) as Action[];
  const runPage: ChordRun = (action, e) => {
    pageBindings[action]?.(e);
  };
  const runAll: ChordRun = (action, e) => {
    const page = pageBindings[action];
    if (page) page(e);
    else repositoryHome?.runChord(action, e);
  };
  /**
   * What a chord that `first` starts on press `e` runs, or null where none starts. No chord starts under a
   * dialog or the palette, which keep their keys, nor on a press that types in a text box. A chord of the
   * page's own works on every screen and in a text box, as a key of one press does. The screen's chords
   * work where the list's keys do, on a Repository's screen outside a text box, and run what the screen runs
   * on the action's key.
   */
  const chordStart = (first: string, e: KeyboardEvent): ChordRun | null => {
    if (!keysActive || (isTextBox(e.target) && typesInTextBox(first))) return null;
    if (home.current?.screen === "repository" && !isTextBox(e.target)) return runAll;
    return startsChordOf(first, pageActions) ? runPage : null;
  };
  /** Reads each key before anything else, so the key that ends a chord's wait goes nowhere else. */
  const readKey = (e: KeyboardEvent) => {
    if (!chord.read(e, chordStart)) guard(e);
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
   * `noteOf` gives it. They come most recently opened first, as `others` does, so the palette's history
   * leaves them out. */
  const switchRows = (
    prefix: string,
    noteOf: (name: string) => string | null = () => null,
  ): PaletteCommand[] =>
    others.map((name) => ({
      title: `${prefix}${name}`,
      note: noteOf(name),
      history: false,
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
  /** The palette's commands: those the user ran, most recently run first, then the others in this order. */
  const commands: PaletteCommand[] = $derived(
    recentCommands.order<PaletteCommand>([
      ...(repositoryHome?.paletteCommands() ?? []),
      ...switchRows("Repository: "),
      { title: "Open Settings", run: openSettings },
      { title: "Pick a Theme", run: openThemePicker },
      { title: "Reload Themes", run: reloadThemes },
      {
        title: hud.visible ? "Hide performance HUD" : "Show performance HUD",
        id: "toggle_hud",
        run: () => {
          hud.toggle();
        },
      },
      // The core says which build it is, so a development UI over a release core lists nothing that fails.
      ...(about.about?.build === "development" ? [{ title: "Open DevTools", run: openDevtools }] : []),
    ]),
  );
</script>

<svelte:window onkeydowncapture={readKey} onpointerdowncapture={endChord} onblur={endChord} />
<!-- A template string: Svelte compiles a bare `{title}` with a `?? ""` branch that no test can reach. -->
<svelte:head><title>{`${title}`}</title></svelte:head>

<main>
  <AboutLine about={about.about} error={about.error} />
  {#if home.current?.screen === "onboarding"}
    <Onboarding token={home.current.token} onadd={openAdd} onsettings={openSettings} />
  {:else if home.current?.screen === "repository"}
    {#if home.switchProblem}
      <p role="alert">{home.switchProblem}</p>
    {/if}
    <RepositoryHome
      bind:this={repositoryHome}
      name={home.current.repository.name}
      onadd={openAdd}
      onpick={openPicker}
      onsettings={openSettings}
      {keysActive}
      diffFiles={settings.diffFiles}
      diffPeek={settings.diffPeek}
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
  problems={[
    ...settings.problems,
    ...theme.problems,
    ...(theme.problem === undefined ? [] : [theme.problem]),
    ...(theme.turnProblem === undefined ? [] : [theme.turnProblem]),
    ...(recentCommands.problem === undefined ? [] : [recentCommands.problem]),
  ]}
  {lastSync}
  syncActivity={status.syncActivity}
  cloning={status.cloning}
  mirrorStates={status.mirrorStates}
  mirrorProblem={status.mirrorProblem}
  mirrorQuestion={status.mirrorQuestion}
  newVersion={status.newVersion}
  chord={chord.first}
  onrefresh={() => void status.refresh()}
  oncheckagain={() => void status.checkMirrorsAgain()}
  onquestion={(repository) => {
    if (!dialogOpen) questionFor = repository;
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
    onrun={(command) => void recentCommands.ran(command)}
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
{#if themePickerOpen}
  <ThemePicker
    {theme}
    onclose={() => {
      themePickerOpen = false;
    }}
    onsaved={() => void settings.load()}
  />
{/if}
{#if settingsOpen}
  <Settings
    {settings}
    onreread={() => {
      void theme.load();
      void recentCommands.load();
    }}
    status={status.status}
    about={about.about}
    repository={openName}
    at={settingsAt}
    onclose={() => {
      settingsOpen = false;
    }}
  />
{/if}
{#if hud.visible}
  <PerformanceHud {hud} refreshing={status.syncActivity !== null} />
{/if}

<style>
  /* The room above the status bar (window.css). A screen taller than the window, such as the first-run one,
     scrolls inside it. */
  main {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 8px;
    overflow: auto;
  }
</style>
