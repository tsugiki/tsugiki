<script lang="ts">
  // PROTOTYPE — throwaway. Keybindings: click a key, press the new one (a chord like Ctrl+K M works:
  // press both within 1.5 s), Esc cancels. Two actions on one key are flagged. Only remapped keys go
  // to config.toml. Prototype: the app itself keeps its built-in keys.
  import { ACTIONS, DEFAULT_KEYS, keyConflicts } from "$lib/core/settings";
  import { config, keyOf, setKey, resetKey } from "$lib/state/settings.svelte";
  import { keyName, prettyKey } from "../keys";
  let { q = "" }: { q?: string } = $props();

  let recording = $state<string | null>(null);
  let first = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  const rows = $derived(ACTIONS.filter((a) => `${a.label} ${prettyKey(keyOf(a.id))} ${a.id}`.toLowerCase().includes(q.toLowerCase().trim())));
  const clash = $derived(new Set(keyConflicts(config().keys).flat()));

  function record(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const k = keyName(e);
    if (!k) return;
    if (k === "escape" && !first) { recording = null; return; }
    clearTimeout(timer);
    if (first) { setKey(recording, `${first} ${k}`); recording = first = null; return; }
    // A Ctrl or Alt combination may start a chord; wait briefly for a second key.
    if (/^(ctrl|alt)\+/.test(k)) {
      first = k;
      const id = recording;
      timer = setTimeout(() => { setKey(id, k); recording = first = null; }, 1500);
    } else { setKey(recording, k); recording = null; }
  }
</script>

<svelte:window onkeydowncapture={record} />

<table>
  <tbody>
    {#each rows as a (a.id)}
      {@const k = keyOf(a.id)}
      <tr class:clash={clash.has(a.id)}>
        <td class="label">{a.label}</td>
        <td>
          <button class="key" class:rec={recording === a.id} onclick={() => { recording = a.id; first = null; }} title="Click, then press the new key">
            {recording === a.id ? (first ? `${prettyKey(first)} …` : "Press a key… (Esc cancels)") : prettyKey(k)}
          </button>
        </td>
        <td class="note">
          {#if clash.has(a.id)}<span class="bad">same key as {ACTIONS.find((b) => b.id !== a.id && keyOf(b.id) === k)?.label}</span>{/if}
          {#if k !== DEFAULT_KEYS[a.id]}<button class="reset" onclick={() => resetKey(a.id)} title="Back to {prettyKey(DEFAULT_KEYS[a.id])}">reset</button>{/if}
        </td>
      </tr>
    {:else}<tr><td class="note">No key matches.</td></tr>{/each}
  </tbody>
</table>

<style>
  table { border-collapse: collapse; width: 100%; font-size: 13px; }
  td { padding: 3px 8px 3px 0; border-bottom: 1px solid var(--line); }
  .label { width: 45%; }
  .key { font: 12px var(--mono); min-width: 120px; text-align: left; }
  .key.rec { border-color: var(--accent); color: var(--accent); }
  .clash .key { border-color: var(--bad); }
  .note { color: var(--muted); font-size: 12px; }
  .bad { color: var(--bad); margin-right: 8px; }
  .reset { font-size: 11px; padding: 0 6px; }
</style>
