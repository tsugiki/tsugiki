<!-- Settings › Keyboard (docs/devel/keyboard.md): every action with its key in force and where it works,
     from the core's key table with the user's remaps, each key `[keys]` remaps marked. A box above the table
     filters its rows by action or by key. Remapping a key with a click comes later (#552). -->
<script lang="ts">
  import { CONTEXT_NAMES, keyTable, type Row } from "$lib/state/keymap.svelte";
  import { bindingLabel, currentOs } from "./keys";
  import StoredIn from "./StoredIn.svelte";

  const os = currentOs();
  let query = $state("");

  /** The box's text, trimmed and in lower case, which each row shown holds. */
  const typed = $derived(query.trim().toLowerCase());
  /** Each action's title, for the row whose default key another action took. */
  const titles = $derived(new Map(keyTable().map((row) => [row.action, row.title])));
  const rows = $derived(keyTable().filter(matches));

  /** Whether `row` holds what the box holds, in any case: in its title, its name in `[keys]`, or its key, by
   * the name `[keys]` writes and the name the page shows, so "ctrl+k" finds Cmd+K M on macOS too. */
  function matches(row: Row): boolean {
    const key = row.key === null ? [] : [row.key, bindingLabel(row.key, os)];
    return [row.title, row.action, ...key].some((text) => text.toLowerCase().includes(typed));
  }

  /** A key as the page shows it, or "no key". */
  const label = (key: string | null) => (key === null ? "no key" : bindingLabel(key, os));

  /** What the mark beside a remapped key says when the pointer rests on it, with the action's default key. */
  const remappedTip = (key: string | null) => `Set in settings.toml's [keys]. Default: ${label(key)}`;

  /** Why a row whose default key another action took has none. */
  const takenWords = (key: string, by: Row["action"]) => `${label(key)} went to ${String(titles.get(by))}`;
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
          {#if row.remapped.kind === "remapped"}
            <span class="remapped" title={remappedTip(row.remapped.default)}>remapped</span>
          {:else if row.remapped.kind === "taken"}
            <span class="taken">{takenWords(row.remapped.default, row.remapped.by)}</span>
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

  table {
    width: 100%;
    border-collapse: collapse;
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
  .taken {
    color: var(--fg-muted);
  }

  .taken {
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
