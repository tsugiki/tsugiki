<!-- The Home page (docs/devel/repositories.md, #564): every Repository on a row of its own, in the Repository
     picker's order, and Add repository… under them. ↑ ↓ move the cursor over the rows and Enter or a click
     opens one on its Workbench. Esc goes back to the Repository the user came from, the top row. `Alt+←` does
     too, as Go Back, which the window runs among its own actions, as `[keys]` may give it another key. -->
<script lang="ts">
  import type { Opened } from "$lib/state/home.svelte";
  import { minuteClock } from "./clock.svelte";
  import { currentOs, keyName, withKey } from "./keys";
  import { agoWords, stampOf } from "./time";

  let {
    rows,
    keysActive,
    onopen,
    onback,
    onadd,
    onsettings,
  }: {
    /** The Repositories, the one the user came from first. */
    rows: readonly Opened[];
    /** Whether the page's keys work: a dialog, the palette or a picker on top keeps them. */
    keysActive: boolean;
    /** Opens the Repository named so on its Workbench. */
    onopen: (name: string) => void;
    /** Goes back to the Repository the user came from. */
    onback: () => void;
    onadd: () => void;
    /** Opens the Settings dialog. */
    onsettings: () => void;
  } = $props();

  const clock = minuteClock();
  /** Each row's words for the time the user last opened its Repository, or nothing for one `recent` has no
   * time for. */
  const opened = $derived(
    rows.map((row) => (row.opened_at === null ? null : `opened ${agoWords(row.opened_at, clock.now)}`)),
  );
  /** Each row's tooltip for that time: the date and time the user last opened its Repository (#1066). */
  const openedTips = $derived(
    rows.map((row) => (row.opened_at === null ? undefined : `Opened ${stampOf(row.opened_at)}`)),
  );
  const os = currentOs();
  /** The name of the row the cursor last went to. */
  let picked = $state<string>();
  /** Where that row last stood. The template reads `cursor` on each change of the rows, so this follows a
   * row that moves before it leaves. */
  let lastAt = 0;
  /** The row under the cursor, which Enter opens: the top one as the page opens, so Enter goes back. It
   * follows its row by name as the rows change, and on a row that leaves, as one a failed switch finds gone,
   * it stays where the row last stood, on the last row at most. */
  const cursor = $derived.by(() => {
    const at = rows.findIndex((row) => row.name === picked);
    if (at === -1) return Math.min(lastAt, rows.length - 1);
    lastAt = at;
    return at;
  });
  const buttons: HTMLButtonElement[] = $state([]);

  // The top row takes the focus as the page opens, so ↑ ↓ and Enter work at once.
  $effect(() => {
    buttons[0]?.focus();
  });

  /** Moves the cursor `by` rows, as the focus moves and takes the cursor with it. */
  function move(by: 1 | -1) {
    buttons[Math.max(0, Math.min(cursor + by, rows.length - 1))]?.focus();
  }

  /** The page's own keys. Enter on a row's button clicks it, so Enter here opens the row under the cursor
   * only while nothing holds the focus, as after a click on the page around the rows. */
  function onkeydown(e: KeyboardEvent) {
    if (!keysActive) return;
    const key = keyName(e, os);
    if (key === "arrowdown" || key === "arrowup") {
      e.preventDefault();
      move(key === "arrowdown" ? 1 : -1);
    } else if (key === "enter" && (e.repeat || e.target === document.body)) {
      // Each opening is a switch, and a held key isn't more than one, so a repeat opens nothing, and the
      // browser doesn't click the focused row again either.
      e.preventDefault();
      const row = rows[cursor];
      if (row && !e.repeat) onopen(row.name);
    } else if (key === "escape") {
      e.preventDefault();
      onback();
    }
  }
</script>

<svelte:window {onkeydown} />

<section>
  <header>
    <h1>Home</h1>
    <button
      class="settings"
      aria-haspopup="dialog"
      aria-label="Settings"
      title={withKey("Settings", "open_settings", os)}
      onclick={onsettings}>⚙ Settings</button
    >
  </header>
  <ul class="row-list" aria-label="Repositories">
    {#each rows as row, at (row.name)}
      <li>
        <button
          bind:this={buttons[at]}
          aria-current={at === cursor ? "true" : undefined}
          onclick={() => {
            onopen(row.name);
          }}
          onfocus={() => {
            picked = row.name;
          }}
        >
          <span class="name mono">{row.name}</span>
          {#if opened[at]}
            <span class="opened" title={openedTips[at]}>{opened[at]}</span>
          {/if}
        </button>
      </li>
    {/each}
  </ul>
  <button class="add" onclick={onadd}>Add repository…</button>
</section>

<style>
  section {
    box-sizing: border-box;
    width: 100%;
    max-width: 860px;
    margin: 0 auto;
    padding: 24px 16px;
  }

  header {
    display: flex;
    gap: 12px;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  h1 {
    margin: 0;
    font-size: 18px;
  }

  li button {
    display: flex;
    gap: 12px;
    align-items: baseline;
    padding: 10px 14px;
  }

  li button[aria-current="true"] {
    background: var(--selected);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .opened {
    font-size: 12px;
    color: var(--fg-muted);
    white-space: nowrap;
  }
</style>
