<!-- Settings › Keyboard (docs/devel/keyboard.md): every action with its key in force and where it works,
     from the core's key table with the user's remaps, each key `[keys]` remaps marked. A box above the table
     filters its rows by action or by key. Remapping a key with a click comes later (#552). -->
<script lang="ts">
  import { CONTEXT_NAMES, keyTable, type Lost, type Remap, type Row } from "$lib/state/keymap.svelte";
  import { bindingLabel, currentOs } from "./keys";
  import StoredIn from "./StoredIn.svelte";

  const os = currentOs();
  let query = $state("");

  /** The box's text, trimmed and in lower case, which each row shown holds. */
  const typed = $derived(query.trim().toLowerCase());
  const rows = $derived(keyTable().filter(matches));

  /** Whether `row` holds what the box holds, in any case: in its title, its name in `[keys]`, its key, or the
   * note on a key another action took. A key matches by the name `[keys]` writes and the name the page shows,
   * so "ctrl+k" finds Cmd+K M on macOS too, and "no key" finds the actions with none. */
  function matches(row: Row): boolean {
    const key = row.key === null ? [] : [row.key];
    const lost = row.lost === null ? [] : [row.lost.key, lostWords(row.lost)];
    return [row.title, row.action, label(row.key), ...key, ...lost].some((text) =>
      text.toLowerCase().includes(typed),
    );
  }

  /** A key as the page shows it, or "No key". */
  function label(key: string | null): string {
    return key === null ? "No key" : bindingLabel(key, os);
  }

  /** What the mark beside a remapped key says when the pointer rests on it, with the action's default key. */
  const remappedTip = ({ default: key }: Remap) => `Set in settings.toml's [keys]. Default: ${label(key)}`;

  /** Why a row has no key though it would have one: which action's line of `[keys]` took it. */
  const lostWords = ({ key, to }: Lost) => `${label(key)} went to ${to}`;

  /** Esc in the box clears it, and closes the dialog only once the box filters nothing. An Esc that cancels
   * an input method's candidates leaves the box to the input method. */
  function clearOnEscape(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.isComposing || typed === "") return;
    query = "";
    e.stopPropagation();
  }
</script>

<p class="help">The default keys follow VS Code's. On macOS, Cmd stands for Ctrl.</p>
<p class="help">
  A line in settings.toml's <code>[keys]</code> remaps a key. <StoredIn where="settings" />
</p>
<input
  class="filter"
  type="search"
  aria-label="Filter keys"
  placeholder="Filter by action or key"
  spellcheck="false"
  bind:value={query}
  onkeydown={clearOnEscape}
/>
<table>
  <thead>
    <tr>
      <th scope="col">Action</th>
      <th scope="col">Key</th>
      <th scope="col">Where it works</th>
    </tr>
  </thead>
  <tbody>
    {#each rows as row (row.action)}
      <tr>
        <td>{row.title}</td>
        <td>
          {#if row.key === null}
            <span class="none">No key</span>
          {:else}
            <kbd>{bindingLabel(row.key, os)}</kbd>
          {/if}
          {#if row.remapped !== null}
            <span class="remapped" title={remappedTip(row.remapped)}>remapped</span>
          {/if}
          {#if row.lost !== null}
            <span class="lost">{lostWords(row.lost)}</span>
          {/if}
        </td>
        <td>{CONTEXT_NAMES[row.context]}</td>
      </tr>
    {:else}
      <tr>
        <td class="none" colspan="3">No action or key holds that.</td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  .help {
    margin: 4px 0;
    font-size: 12px;
    color: var(--fg-muted);
  }

  .filter {
    width: 100%;
    margin: 8px 0;
    box-sizing: border-box;
  }

  /* Separate, as a sticky header cell of a collapsed table loses its bottom border in Chromium once the table
     scrolls. */
  table {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
  }

  th {
    position: sticky;
    top: 0;
    font-weight: 600;
    text-align: left;
    background: var(--bg-overlay);
  }

  th,
  td {
    padding: 3px 8px 3px 0;
    border-bottom: 1px solid var(--border);
  }

  .none,
  .lost {
    color: var(--fg-muted);
  }

  .lost {
    margin-left: 6px;
    font-size: 11.5px;
  }

  .remapped {
    margin-left: 6px;
    padding: 0 6px;
    font-size: 10.5px;
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 999px;
  }
</style>
