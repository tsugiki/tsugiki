<script lang="ts" module>
  import type { PaletteId } from "$lib/state/recent-commands.svelte";

  /** A row: one that runs, or one that shows greyed and runs nothing, such as a theme file that can't be
   * used, whose note says why. */
  export type PaletteCommand = PaletteAction | (PaletteRow & { disabled: true; run?: never });

  /** A row that runs. */
  export type PaletteAction = PaletteRow & { run: () => void; disabled?: false };

  interface PaletteRow {
    title: string;
    /** What the command palette's history knows the row by where its title changes with what it does.
     * Without one it goes by its title. */
    id?: PaletteId;
    /** Keeps the row out of the command palette's history, as a switch to a Repository. */
    history?: false;
    /** Marks the row that stands for what is open now, such as the open Repository in the picker. */
    current?: boolean;
    /** Words after the title, such as why a Repository's diffs aren't there yet. Typing doesn't match
     * them. */
    note?: string | null;
    /** Runs while the row is the one picked, such as a theme that shows on the window before it's chosen. */
    preview?: () => void;
    /** Runs without closing, such as the theme picker's Reload themes. */
    keepsOpen?: boolean;
    /** The heading the row lists under, such as a theme's side in the theme picker. Rows next to each other
     * with the same heading list under one. */
    group?: string | undefined;
  }

  /** What a quick pick calls itself, its text box, its list, a list with nothing in it, and the current
   * row: "open" unless it says. */
  export interface PaletteWords {
    dialog: string;
    input: string;
    placeholder: string;
    list: string;
    none: string;
    mark?: string;
  }

  /** Quick open's answer to a text typed: its rows, or why it has none. */
  export type QuickOpenRows = { typed: string } & (
    { rows: readonly PaletteCommand[]; problem?: never } | { rows?: never; problem: string }
  );

  const COMMAND_WORDS: PaletteWords = {
    dialog: "Command palette",
    input: "Command",
    placeholder: "Type a command",
    list: "Commands",
    none: "No command matches",
  };

  const QUICK_OPEN_WORDS: PaletteWords = {
    dialog: "Quick open",
    input: "Issue or PR",
    placeholder: "Type an Issue's or a PR's number, or words from its title; > for commands",
    list: "Issues and PRs",
    none: "No Issue or PR matches",
  };
</script>

<script lang="ts">
  // The command palette: it lists the commands that hold what the user typed, in the order it was given them,
  // and runs the one they pick. Each feature brings its own commands, and the page puts those the user ran
  // last first (#556). The Repository picker is the same quick pick with other words and rows.
  //
  // Given `find`, it is quick open too, as in VS Code (#557): a text that doesn't start with `>` asks `find`
  // for the Issues and PRs it names, and lists the rows `found` brings back, in the order they come.
  import { tick, untrack } from "svelte";
  import { closeOnPress, takeFocus } from "./focus";

  let {
    commands,
    onclose,
    oncancel = () => {},
    onrun = () => {},
    words = COMMAND_WORDS,
    start,
    text = "",
    find,
    found,
  }: {
    commands: readonly PaletteCommand[];
    onclose: () => void;
    /** Runs before `onclose` when it closes without running a row: on Esc or a press outside it. */
    oncancel?: () => void;
    /** Runs as a row runs, before the row does, such as the command palette's note that the user ran it. */
    onrun?: (command: PaletteAction) => void;
    words?: PaletteWords;
    /** The row picked when it opens. Without it the pick is on the first row, and stays on whichever row
     * is first until the user moves it: the command palette's order can change while it's open, as the
     * history arrives. Typing picks the first. */
    start?: number;
    /** What the text box holds when it opens, such as the `>` the command palette opens with. */
    text?: string;
    /** Asks quick open what a text typed finds. Without it, the palette has no quick open. */
    find?: ((typed: string) => unknown) | undefined;
    /** Quick open's latest answer. */
    found?: QuickOpenRows | undefined;
  } = $props();

  let typed = $state(untrack(() => text));
  // The picked row goes by its title, so a list that changes while open, as Reload themes does, keeps the
  // pick on the same row. No title, or one the list no longer holds, picks the first.
  let pickedTitle = $state(untrack(() => (start === undefined ? undefined : commands[start]?.title)));
  /** Whether the palette lists quick open's rows: it has quick open, and the text doesn't start with `>`. */
  const quick = $derived(find !== undefined && !typed.startsWith(">"));
  /** Whether quick open's answer to the text in the box hasn't come yet. The last answer shows meanwhile. */
  const waiting = $derived(quick && found?.typed !== typed);
  /** What the commands are filtered by: the text, after the `>` that marks the commands, if it has quick
   * open. */
  const filter = $derived(
    (find !== undefined && typed.startsWith(">") ? typed.slice(1) : typed).trim().toLowerCase(),
  );
  const matching = $derived(
    quick ? (found?.rows ?? []) : commands.filter((command) => command.title.toLowerCase().includes(filter)),
  );
  /** The rows listed, in runs from `start`, their index in `matching`: rows next to each other with the same
   * `group` list under one heading, and rows without one under none. */
  const sections = $derived(
    matching.reduce<{ group: string | undefined; start: number; rows: PaletteCommand[] }[]>(
      (all, command, index) => {
        const last = all.at(-1);
        if (last && last.group === command.group) last.rows.push(command);
        else all.push({ group: command.group, start: index, rows: [command] });
        return all;
      },
      [],
    ),
  );
  const shownWords = $derived(quick ? QUICK_OPEN_WORDS : words);
  // Each text quick open shows asks it once.
  $effect(() => {
    if (!quick) return;
    const asked = typed;
    untrack(() => find?.(asked));
  });
  /** Whether Enter came before quick open's answer to the text in the box, and runs the first row of that
   * answer once it comes. A fast typist's `123` and Enter then opens #123, and not what `12` found. Typing
   * on or moving the pick forgets it. */
  let enterWaits = $state(false);
  $effect(() => {
    if (enterWaits && !waiting) {
      enterWaits = false;
      untrack(() => {
        if (pickedCommand) run(pickedCommand);
      });
    }
  });
  const picked = $derived(
    Math.max(
      0,
      matching.findIndex((command) => command.title === pickedTitle),
    ),
  );
  const pickedCommand = $derived(matching[picked]);
  // By its title, so rows built again as they were don't preview again, after a pick has run, say.
  const shownTitle = $derived(pickedCommand?.title);
  $effect(() => {
    if (shownTitle !== undefined) untrack(() => pickedCommand?.preview?.());
  });

  /** Closes, and runs `command` once the palette has gone and handed the focus back, so the command finds
   * the focus where the user left it: a pane command then moves it as its key would. A row that keeps it
   * open runs at once. */
  function run(command: PaletteCommand) {
    if (command.disabled) return;
    onrun(command);
    if (command.keepsOpen) {
      command.run();
      return;
    }
    onclose();
    void tick().then(command.run);
  }

  function cancel() {
    oncancel();
    onclose();
  }

  function pick(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      // The user picks a row themselves now, which Enter then runs.
      enterWaits = false;
      const next = Math.max(Math.min(picked + (e.key === "ArrowDown" ? 1 : -1), matching.length - 1), 0);
      pickedTitle = matching[next]?.title;
    } else if (e.key === "Enter") {
      // Closing hands the focus back, as to the Sidebar's button that opened the Repository picker, and
      // Chromium clicks a focused button on the keypress after this keydown, which would open it again.
      e.preventDefault();
      if (waiting) enterWaits = true;
      else if (pickedCommand) run(pickedCommand);
    }
  }

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") {
      cancel();
    }
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. It goes by the press, because a
     drag that starts in the text box and ends out here also counts as a click out here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="palette-scrim" onmousedown={closeOnPress(cancel)}>
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label={shownWords.dialog}
    tabindex="-1"
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <input
      {@attach takeFocus}
      bind:value={typed}
      oninput={() => {
        pickedTitle = undefined;
        enterWaits = false;
      }}
      onkeydown={pick}
      role="combobox"
      aria-expanded={matching.length > 0}
      aria-controls="palette-commands"
      aria-activedescendant={matching.length > 0 ? `palette-command-${String(picked)}` : undefined}
      aria-label={shownWords.input}
      placeholder={shownWords.placeholder}
    />
    {#snippet option(command: PaletteCommand, i: number)}
      <!-- The keys pick from the text box, which keeps the focus, so a click is the mouse's way. -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li
        id={`palette-command-${String(i)}`}
        role="option"
        aria-selected={i === picked}
        aria-current={command.current ? "true" : undefined}
        aria-disabled={command.disabled ? "true" : undefined}
        onclick={() => {
          run(command);
        }}
      >
        {command.title}{#if command.note}<span class="note">{` · ${command.note}`}</span
          >{/if}{#if command.current}&nbsp;<span class="note open">{words.mark ?? "open"}</span>{/if}
      </li>
    {/snippet}
    {#if matching.length > 0}
      <ul id="palette-commands" role="listbox" aria-label={shownWords.list}>
        {#each sections as section (section.start)}
          {#if section.group === undefined}
            {#each section.rows as command, i (command.title)}
              {@render option(command, section.start + i)}
            {/each}
          {:else}
            <!-- The group's label names it to a screen reader, so the heading is only for the eye. -->
            <li class="group" role="group" aria-label={section.group}>
              <div class="heading" aria-hidden="true">{section.group}</div>
              <ul role="none">
                {#each section.rows as command, i (command.title)}
                  {@render option(command, section.start + i)}
                {/each}
              </ul>
            </li>
          {/if}
        {/each}
      </ul>
    {:else if quick && found?.problem !== undefined}
      <p id="palette-commands" role="alert">{found.problem}</p>
    {:else if !waiting}
      <p id="palette-commands">{shownWords.none}</p>
    {/if}
  </div>
</div>

<style>
  .palette {
    width: min(640px, calc(100vw - 32px));
    margin: 60px auto 0;
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 30px var(--shadow);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    font-size: 14px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 6px 6px 0 0;
    color: inherit;
    background: transparent;
  }

  /* The line under the box takes the accent with the focus, as other inputs' borders do. */
  input:focus {
    border-bottom-color: var(--accent);
  }

  ul {
    margin: 0;
    padding: 4px 0;
    list-style: none;
  }

  li {
    padding: 4px 10px;
    font-size: 13px;
    cursor: default;
  }

  li[aria-selected="true"] {
    background: var(--selected);
  }

  /* A heading over its rows, with a line between it and the rows above. */
  .group {
    padding: 0;
  }

  .group > ul {
    padding: 0;
  }

  .group:not(:first-child),
  .group + li {
    margin-top: 4px;
    border-top: 1px solid var(--border);
  }

  .heading {
    padding: 6px 10px 2px;
    font-size: 11px;
    color: var(--fg-muted);
  }

  /* Greyed text, as muted text is: its note says why it can't run. */
  li[aria-disabled="true"] {
    color: var(--fg-muted);
  }

  .note {
    color: var(--fg-muted);
  }

  .open {
    float: right;
  }

  p {
    margin: 0;
    padding: 8px 10px;
    font-size: 13px;
    color: var(--fg-muted);
  }
</style>
