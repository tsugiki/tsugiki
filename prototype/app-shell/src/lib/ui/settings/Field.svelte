<script lang="ts">
  // PROTOTYPE — throwaway. The control for one setting, by type. Shared by the variants; the layout
  // around it is each variant's own.
  import type { SettingDef } from "$lib/core/settings";
  import { value, setValue, secret } from "$lib/state/settings.svelte";
  let { def, radios = false }: { def: SettingDef; radios?: boolean } = $props();
  const t = $derived(def.type);
  const v = $derived(value(def.key));
  const id = $derived(`f-${def.key}`);
  const disabled = $derived(def.key === "auth.token" && value("auth.source") !== "token");
</script>

{#if t.kind === "bool"}
  <label class="check"><input {id} type="checkbox" checked={v as boolean} onchange={(e) => setValue(def.key, e.currentTarget.checked)} /> {def.label}</label>
{:else if t.kind === "number"}
  <span class="num"><input {id} type="number" min={t.min} max={t.max} value={v as number}
    onchange={(e) => { const n = +e.currentTarget.value; if (n >= t.min && n <= t.max) setValue(def.key, n); else e.currentTarget.value = String(v); }} />
    {#if t.unit}<span class="unit">{t.unit}</span>{/if}</span>
{:else if t.kind === "enum" && radios}
  <span class="radios">{#each t.options as [o, label]}<label><input type="radio" name={id} checked={v === o} onchange={() => setValue(def.key, o)} /> {label}</label>{/each}</span>
{:else if t.kind === "enum"}
  <select {id} value={v as string} onchange={(e) => setValue(def.key, e.currentTarget.value)}>
    {#each t.options as [o, label]}<option value={o}>{label}</option>{/each}
  </select>
{:else if t.kind === "secret"}
  <input {id} class="text" type="password" {disabled} placeholder={disabled ? "Not used with the gh CLI token" : "ghp_… or github_pat_…"} bind:value={secret.token} autocomplete="off" />
{:else}
  <input {id} class="text" class:mono={t.mono} value={v as string} onchange={(e) => setValue(def.key, e.currentTarget.value)} spellcheck="false" />
{/if}

<style>
  input, select { font: 13px var(--sans); color: var(--fg); background: var(--bg); border: 1px solid var(--line); border-radius: 4px; padding: 3px 6px; }
  input:focus, select:focus { outline: none; border-color: var(--accent); }
  input[type="number"] { width: 80px; }
  input[type="checkbox"], input[type="radio"] { padding: 0; }
  .text { width: min(440px, 100%); box-sizing: border-box; }
  .mono { font-family: var(--mono); font-size: 12.5px; }
  .num { display: inline-flex; gap: 6px; align-items: center; }
  .unit { color: var(--muted); font-size: 12px; }
  .check { display: inline-flex; gap: 6px; align-items: center; font-size: 13px; cursor: pointer; }
  .radios { display: inline-flex; flex-direction: column; gap: 4px; font-size: 13px; }
  .radios label { display: inline-flex; gap: 6px; align-items: center; cursor: pointer; }
  input:disabled { opacity: 0.5; }
</style>
