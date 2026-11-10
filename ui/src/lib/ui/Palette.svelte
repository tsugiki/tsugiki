<script lang="ts" module>
  export interface PaletteCommand {
    title: string;
    run: () => void;
    /** Marks the row that stands for what is open now, such as the open Repository in the picker. */
    current?: boolean;
    /** Words after the title, such as why a Repository's diffs aren't there yet. Typing doesn't match
     * them. */
    note?: string | null;
  }

  /** What a quick pick calls itself, its text box, its list, and a list with nothing in it. */
  export interface PaletteWords {
    dialog: string;
    input: string;
    placeholder: string;
    list: string;
    none: string;
  }

  const COMMAND_WORDS: PaletteWords = {
    dialog: "Command palette",
    input: "Command",
    placeholder: "Type a command",
    list: "Commands",
    none: "No command matches",
  };
</script>

<script lang="ts">
  // The command palette: it lists the commands that hold what the user typed, and runs the one they pick.
  // Each feature brings its own commands, and the ranking comes with the whole palette (docs/build-order.md).
  // The Repository picker is the same quick pick with other words and rows.
  import { untrack } from "svelte";
  import { closeOnPress, takeFocus } from "./focus";

  let {
    commands,
    onclose,
    words = COMMAND_WORDS,
    start = 0,
  }: {
    commands: readonly PaletteCommand[];
    onclose: () => void;
    words?: PaletteWords;
    /** The row picked when it opens. Typing picks the first. */
    start?: number;
  } = $props();

  let typed = $state("");
  let picked = $state(untrack(() => start));
  const matching = $derived(
    commands.filter((command) => command.title.toLowerCase().includes(typed.trim().toLowerCase())),
  );

  function run(command: PaletteCommand) {
    onclose();
    command.run();
  }

  function pick(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      picked = Math.max(Math.min(picked + (e.key === "ArrowDown" ? 1 : -1), matching.length - 1), 0);
    } else if (e.key === "Enter") {
      const command = matching[picked];
      if (command) run(command);
    }
  }

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onclose();
    }
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. It goes by the press, because a
     drag that starts in the text box and ends out here also counts as a click out here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid="palette-scrim" onmousedown={closeOnPress(onclose)}>
  <div
    class="palette"
    role="dialog"
    aria-modal="true"
    aria-label={words.dialog}
    tabindex="-1"
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    <input
      {@attach takeFocus}
      bind:value={typed}
      oninput={() => {
        picked = 0;
      }}
      onkeydown={pick}
      role="combobox"
      aria-expanded={matching.length > 0}
      aria-controls="palette-commands"
      aria-activedescendant={matching.length > 0 ? `palette-command-${String(picked)}` : undefined}
      aria-label={words.input}
      placeholder={words.placeholder}
    />
    {#if matching.length > 0}
      <ul id="palette-commands" role="listbox" aria-label={words.list}>
        {#each matching as command, i (command.title)}
          <!-- The keys pick from the text box, which keeps the focus, so a click is the mouse's way. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={`palette-command-${String(i)}`}
            role="option"
            aria-selected={i === picked}
            aria-current={command.current ? "true" : undefined}
            onclick={() => {
              run(command);
            }}
          >
            {command.title}{#if command.note}<span class="note">{` · ${command.note}`}</span
              >{/if}{#if command.current}&nbsp;<span class="note open">open</span>{/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p id="palette-commands">{words.none}</p>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 15%);
  }

  .palette {
    width: min(640px, calc(100vw - 32px));
    margin: 60px auto 0;
    background: #fff;
    border: 1px solid #ccc;
    border-radius: 6px;
    box-shadow: 0 8px 30px rgb(0 0 0 / 25%);
  }

  input {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    font-size: 14px;
    border: 0;
    border-bottom: 1px solid #ccc;
    border-radius: 6px 6px 0 0;
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
    background: #e4ecf7;
  }

  .note {
    color: #555;
  }

  .open {
    float: right;
  }

  p {
    margin: 0;
    padding: 8px 10px;
    font-size: 13px;
    color: #555;
  }
</style>
