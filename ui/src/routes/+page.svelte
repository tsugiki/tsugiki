<script lang="ts">
  import { untrack } from "svelte";
  import { AboutState, openDevtools } from "$lib/state/about.svelte";
  import { HomeState } from "$lib/state/home.svelte";
  import {
    type Action,
    chordFirst,
    isChord,
    isChordStart,
    keyTable,
    type Row,
    startsChordOf,
  } from "$lib/state/keymap.svelte";
  import { hud } from "$lib/state/hud.svelte";
  import {
    type ChangedFiles,
    filesFound,
    type QuickOpenItem,
    QuickOpenState,
  } from "$lib/state/quick-open.svelte";
  import { RecentCommands } from "$lib/state/recent-commands.svelte";
  import type { ServerName } from "$lib/state/server-words";
  import { settingsAsk } from "$lib/state/settings-ask.svelte";
  import { SettingsState } from "$lib/state/settings.svelte";
  import { StatusState } from "$lib/state/status.svelte";
  import { applyTheme, ThemeState } from "$lib/state/theme.svelte";
  import AboutLine from "$lib/ui/AboutLine.svelte";
  import AddRepository from "$lib/ui/AddRepository.svelte";
  import { type ChordRun, ChordWait } from "$lib/ui/chord.svelte";
  import KeyList, { type KeysHere } from "$lib/ui/KeyList.svelte";
  import {
    type Bindings,
    currentOs,
    isTextBox,
    keyGuard,
    keyTarget,
    type KeyTarget,
    typesInTextBox,
  } from "$lib/ui/keys";
  import { mirrorNote } from "$lib/ui/mirror-note";
  import { headline } from "$lib/ui/workbench-row";
  import MirrorQuestionDialog from "$lib/ui/MirrorQuestionDialog.svelte";
  import Onboarding from "$lib/ui/Onboarding.svelte";
  import { issueIcon, prIcon } from "$lib/ui/octicons";
  import Palette, {
    type PaletteAction,
    type PaletteCommand,
    type PaletteName,
    type PaletteWords,
    type QuickOpenRows,
  } from "$lib/ui/Palette.svelte";
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
  /** What the palette's text box holds as it opens, while the palette is open: `>` for its commands, or
   * nothing for quick open. */
  let paletteText = $state<string>();
  /** Where the focus was as the palette last opened: the palette lists the actions that can run there, and
   * runs them as their keys would (#555). It holds no element, so it stays after the palette closes, and the
   * palette's rows can read it until they go. */
  let paletteAt = $state.raw<KeyTarget>(keyTarget(null));
  /** Quick open's search, a new one each time the palette opens on a Repository's screen. */
  let quickOpen = $state<QuickOpenState>();
  let pickerOpen = $state(false);
  let themePickerOpen = $state(false);
  let settingsOpen = $state(false);
  /** Which keys the `?` list shows, while it's open. */
  let keysHere = $state<KeysHere>();
  /** The Repository whose mirror's question the dialog asks, while it's open. */
  let questionFor = $state<string>();
  /** Whether a dialog, the palette or a picker is open. Each closes on Esc, so one on top of another would
   * lose what the user typed below, and none opens over another: a control behind one, which a press or Tab
   * can still reach, does nothing. A command run from the palette runs once it has closed. */
  const dialogOpen = $derived(
    addOpen ||
      paletteText !== undefined ||
      pickerOpen ||
      themePickerOpen ||
      settingsOpen ||
      keysHere !== undefined ||
      questionFor !== undefined,
  );
  const openAdd = () => {
    if (!dialogOpen) addOpen = true;
  };
  /** Whether a Repository's screen shows. */
  const onRepository = () => home.current?.screen === "repository";
  /** Opens the Repository picker, which only a Repository's screen has: its button is the screen's, and its
   * key's `when` is `onRepository`. */
  const openPicker = () => {
    if (!dialogOpen) pickerOpen = true;
  };
  /** Opens the palette with `text` in its box, unless a dialog is open. On a Repository's screen it has
   * quick open too, which searches that Repository (#557): `Ctrl+P` opens it with nothing typed, and
   * `Ctrl+Shift+P` with the `>` that lists the commands. With a PR on show, quick open finds its files too
   * (#558). Elsewhere it lists only the commands, with no `>`. */
  const openPalette = (text: "" | ">") => {
    if (dialogOpen) return;
    quickOpen =
      openName === undefined ? undefined : new QuickOpenState(openName, repositoryHome?.changedFiles());
    paletteAt = keyTargetOf(document.activeElement);
    paletteText = quickOpen === undefined ? "" : text;
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
  /** One of the page's actions: what it runs for a press `at`, and `when` it can run at all, if not always,
   * which the keys, the `?` list and the palette ask. A dialog on top stops most of them too, as each says.
   * `palette` names its row in the command palette, false for none, or else the key table's title does. */
  interface PageAction {
    when?: () => boolean;
    run: (at: KeyTarget) => void;
    palette?: false | (() => PaletteName);
  }
  /** The actions the page runs, on a key of one press or a chord, as `[keys]` may give either (#549), or from
   * the palette (#555). */
  const PAGE_ACTIONS = new Map<Action, PageAction>([
    // The palette's own two keys open the box it is, so it doesn't list them.
    [
      "command_palette",
      {
        run: () => {
          openPalette(">");
        },
        palette: false,
      },
    ],
    // Quick open searches one Repository, so the Home page has none (#371).
    [
      "quick_open",
      {
        when: onRepository,
        run: () => {
          openPalette("");
        },
        palette: false,
      },
    ],
    ["switch_repository", { when: onRepository, run: openPicker }],
    ["open_settings", { run: openSettings }],
    [
      "toggle_hud",
      {
        run: () => {
          hud.toggle();
        },
        palette: () => ({
          title: hud.visible ? "Hide performance HUD" : "Show performance HUD",
          id: "toggle_hud",
        }),
      },
    ],
    // The webview opens DevTools on the key itself, which the guard leaves alone (ADR 0013), so this runs
    // from the palette only. The core says which build it is, so a development UI over a release core lists
    // nothing that fails.
    [
      "open_devtools",
      {
        when: () => about.about?.build === "development",
        run: () => void openDevtools(),
      },
    ],
    [
      "pick_theme",
      {
        run: () => {
          if (!dialogOpen) openThemePicker();
        },
      },
    ],
    ["reload_themes", { run: reloadThemes }],
    // The list shows the keys that work where the press went, so it asks before it takes the focus.
    [
      "show_keys",
      {
        run: (at) => {
          if (!dialogOpen) keysHere = keysAt(at);
        },
      },
    ],
  ]);
  /** Whether `action` can run now, as far as the page says: its `when`, if it is the page's and has one. */
  const pageCanRun = (action: Action) => PAGE_ACTIONS.get(action)?.when?.() !== false;
  /** Where a press on `target` went: the Repository's screen says, and elsewhere no pane has it. */
  const keyTargetOf = (target: EventTarget | null) => keyTarget(target, repositoryHome?.paneOf);
  const pageBindings: Bindings = Object.fromEntries(
    [...PAGE_ACTIONS].map(([action, { run }]) => [
      action,
      (e: KeyboardEvent) => {
        if (pageCanRun(action)) run(keyTargetOf(e.target));
      },
    ]),
  );
  const guard = keyGuard(pageBindings, os);
  const pageActions = [...PAGE_ACTIONS.keys()];
  /**
   * Which keys work for a press `at`, for the `?` list, worked out now, as the press would run them. The
   * screen says which of its actions would run, and the page which of its own. Off a Repository's screen
   * only the page's own work. A press reaches its action as `chordStartAt` and the guard let it: a chord only
   * where it starts, and a key of one press only where no chord's wait takes it and no text box keeps it.
   */
  function keysAt(at: KeyTarget): KeysHere {
    const onScreen = repositoryHome?.keysHere(at) ?? {
      views: [],
      works: (action: Action) => PAGE_ACTIONS.has(action),
    };
    const reaches = (key: string, action: Action) => {
      if (!isChord(key)) {
        const kept = at.textBox && typesInTextBox(key);
        return !kept && (!isChordStart(key) || chordStartAt(key, at.textBox) === null);
      }
      const run = chordStartAt(chordFirst(key), at.textBox);
      return run === runAll || (run === runPage && PAGE_ACTIONS.has(action));
    };
    const works = (row: Row) =>
      row.key !== null &&
      reaches(row.key, row.action) &&
      onScreen.works(row.action) &&
      pageCanRun(row.action);
    return {
      views: onScreen.views,
      actions: new Set(keyTable().flatMap((row) => (works(row) ? [row.action] : []))),
    };
  }
  const runPage: ChordRun = (action, e) => {
    pageBindings[action]?.(e);
  };
  const runAll: ChordRun = (action, e) => {
    const page = pageBindings[action];
    if (page) page(e);
    else repositoryHome?.runChord(action, e);
  };
  /**
   * What a chord that `first` starts runs, on a press in a text box if `textBox` says so, or null where none
   * starts. No chord starts under a dialog or the palette, which keep their keys, nor on a press that types
   * in a text box. A chord of the page's own works on every screen and in a text box, as a key of one press
   * does. The screen's chords work where the list's keys do, on a Repository's screen outside a text box,
   * and run what the screen runs on the action's key.
   */
  const chordStartAt = (first: string, textBox: boolean): ChordRun | null => {
    if (!keysActive || (textBox && typesInTextBox(first))) return null;
    if (onRepository() && !textBox) return runAll;
    return startsChordOf(first, pageActions) ? runPage : null;
  };
  /** Reads each key before anything else, so the key that ends a chord's wait goes nowhere else. */
  const readKey = (e: KeyboardEvent) => {
    if (!chord.read(e, (first) => chordStartAt(first, isTextBox(e.target)))) guard(e);
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
  /** Quick open's row for `item`: its number and title, with what it is and its state beside them. It keeps
   * out of the palette's history, which holds commands. */
  const foundRow = (item: QuickOpenItem): PaletteCommand => ({
    title: headline(item),
    key: `item ${String(item.number)}`,
    note: item.kind === "issue" ? (issueIcon(item.state)?.label ?? "Issue") : prIcon(item).label,
    history: false,
    run: () => {
      repositoryHome?.openFound(item.kind, item.number);
    },
  });
  /** Quick open's row for the file at `path` in the PR on show, which opens it in the Files tab (#558). */
  const fileRow = (files: ChangedFiles, path: string): PaletteCommand => ({
    title: path,
    key: `file ${path}`,
    note: "File",
    history: false,
    run: () => {
      files.open(path);
    },
  });
  /** Quick open's rows for the files of the PR on show that `typed` finds, and whether its diff that holds
   * them is still on its way. */
  const fileRows = (files: ChangedFiles | undefined, typed: string) => {
    if (files === undefined) return { rows: [], pending: false };
    const paths = filesFound(files, typed);
    return { rows: (paths ?? []).map((path) => fileRow(files, path)), pending: paths === undefined };
  };
  /** Quick open's latest answer, as the palette's rows: the Issue or PR the text names by its number, the
   * files of the PR on show that the text finds, then the other Issues and PRs (#558). Files need no search
   * of the core's, so they show when it fails too. The item the number names heads the rows whatever files
   * come, so an Enter waits for no diff to open it. */
  const foundRows = $derived.by((): QuickOpenRows | undefined => {
    const search = quickOpen;
    const answer = search?.answer;
    if (search === undefined || answer === undefined) return undefined;
    const files = fileRows(search.files, answer.typed);
    if ("problem" in answer) return { typed: answer.typed, ...files, problem: answer.problem };
    const { named, others } = answer.found;
    return {
      typed: answer.typed,
      rows: [...(named ? [foundRow(named)] : []), ...files.rows, ...others.map(foundRow)],
      pending: !named && files.pending,
    };
  });
  /** The palette's row for `row`'s action, if it is the page's and can run, named as its `palette` says,
   * which runs what its key runs `at` the focus as the palette opened (#555). */
  const pageRow = ({ action, title }: Row, at: KeyTarget): PaletteAction | undefined => {
    const page = PAGE_ACTIONS.get(action);
    if (page === undefined || page.palette === false || !pageCanRun(action)) return undefined;
    return {
      ...(page.palette?.() ?? { title }),
      action,
      run: () => {
        page.run(at);
      },
    };
  };
  /** The palette's commands for the focus `at`: those the user ran, most recently run first, then the others
   * in their usual order: each action that can run there, the page's or the screen's, in the key table's
   * order, then the screen's commands that run no action, then a switch to each other Repository. */
  const paletteCommands = (at: KeyTarget): PaletteCommand[] =>
    recentCommands.order<PaletteCommand>([
      ...keyTable().flatMap((row) => pageRow(row, at) ?? repositoryHome?.paletteRow(row, at) ?? []),
      ...(repositoryHome?.paletteCommands() ?? []),
      ...switchRows("Repository: "),
    ]);
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
{#if paletteText !== undefined}
  <Palette
    commands={paletteCommands(paletteAt)}
    text={paletteText}
    find={quickOpen?.find}
    found={foundRows}
    files={quickOpen?.files !== undefined}
    onrun={(command) => void recentCommands.ran(command)}
    onclose={() => {
      paletteText = undefined;
      quickOpen?.stop();
      quickOpen = undefined;
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
    {theme}
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
{#if keysHere !== undefined}
  <KeyList
    here={keysHere}
    onclose={() => {
      keysHere = undefined;
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
