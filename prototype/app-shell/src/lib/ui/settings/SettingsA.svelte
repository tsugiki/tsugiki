<script lang="ts">
  // PROTOTYPE — throwaway. Variant A, "Searchable list": the whole window, like VS Code's settings
  // editor. One long list of every setting with its description, a contents column to jump, and a
  // search box that narrows everything (settings, keys, Repositories). Changes apply at once.
  import { ACTIONS, SECTIONS, SETTINGS, searchSettings } from "$lib/core/settings";
  import { isDefault, resetValue, settingsUi, keyOf } from "$lib/state/settings.svelte";
  import { prettyKey } from "../keys";
  import { REPOS, wb } from "$lib/state/app.svelte";
  import Field from "./Field.svelte";
  import Where from "./Where.svelte";
  import KeysTable from "./KeysTable.svelte";
  import RepoForm from "./RepoForm.svelte";
  import Files from "./Files.svelte";
  let { onClose }: { onClose: () => void } = $props();

  let q = $state("");
  let showFiles = $state(true);
  let list = $state<HTMLElement>();
  const hits = $derived(q.trim() ? searchSettings(q) : SETTINGS);
  const repos = $derived(REPOS.filter((r) => !q.trim() || "repositories repository filters pinned muted clone conventions".includes(q.toLowerCase().trim()) || r.includes(q.toLowerCase().trim())));
  const keysQ = $derived(/^(keys?|shortcuts?|keybindings?)$/i.test(q.trim()) ? "" : q);
  const keysShown = $derived(!keysQ.trim() || ACTIONS.some((a) => `${a.label} ${prettyKey(keyOf(a.id))} ${a.id}`.toLowerCase().includes(keysQ.toLowerCase().trim())));
  const jump = (id: string) => list?.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({ block: "start" });
  $effect(() => { if (settingsUi.section) queueMicrotask(() => jump(settingsUi.section!.startsWith("repo:") ? settingsUi.section! : `s-${settingsUi.section}`)); });
</script>

<div class="a" class:files={showFiles}>
  <header>
    <b>Settings</b>
    <!-- svelte-ignore a11y_autofocus -->
    <input class="search" bind:value={q} placeholder="Search settings, keys, Repositories" autofocus />
    <span class="grow"></span>
    <label class="tog"><input type="checkbox" bind:checked={showFiles} /> Show files</label>
    <button onclick={onClose} title="Close (Esc)">✕</button>
  </header>
  <nav class="toc">
    {#each SECTIONS as s}<button onclick={() => jump(`s-${s}`)} disabled={!hits.some((h) => h.section === s)}>{s}</button>{/each}
    <button onclick={() => jump("s-Keybindings")}>Keybindings</button>
    <button onclick={() => jump("s-Repositories")} disabled={!repos.length}>Repositories</button>
    {#each repos as r}<button class="sub" onclick={() => jump(`repo:${r}`)}>{r}</button>{/each}
  </nav>
  <main bind:this={list}>
    {#each SECTIONS as s}
      {@const rows = hits.filter((h) => h.section === s)}
      {#if rows.length}
        <h2 id="s-{s}">{s}</h2>
        {#if s === "Account"}<p class="status">Signed in as <b>{wb.me}</b> with the gh CLI's token.</p>{/if}
        {#each rows as d (d.key)}
          <div class="set" class:mod={!isDefault(d.key)}>
            <div class="title"><span class="sec">{d.section}:</span> <b>{d.label}</b> <Where where={d.where} />
              {#if !isDefault(d.key) && d.where !== "keychain"}<button class="reset" onclick={() => resetValue(d.key)} title="Reset to the default">↺ reset</button>{/if}
              <span class="key">{d.key}</span></div>
            <div class="help">{d.help}</div>
            <Field def={d} />
          </div>
        {/each}
      {/if}
    {/each}
    {#if keysShown}
      <h2 id="s-Keybindings">Keybindings <Where where="synced" /></h2>
      <p class="help">VS Code's keys by default; on macOS Cmd stands for Ctrl. Click a key to change it.</p>
      <KeysTable q={keysQ} />
    {/if}
    {#if repos.length}
      <h2 id="s-Repositories">Repositories</h2>
      {#each repos as r}
        <h3 id="repo:{r}">{r}</h3>
        <RepoForm name={r} />
      {/each}
    {/if}
    {#if !hits.length && !repos.length && !keysShown}<p class="help">No setting matches “{q}”.</p>{/if}
  </main>
  {#if showFiles}<aside><Files /></aside>{/if}
</div>

<style>
  .a { position: fixed; inset: 0; z-index: 40; background: var(--bg); display: grid; grid-template-columns: 210px 1fr; grid-template-rows: auto 1fr; font-size: 13px; }
  .a.files { grid-template-columns: 210px 1fr 440px; }
  header { grid-column: 1 / -1; display: flex; gap: 12px; align-items: center; padding: 8px 14px; border-bottom: 1px solid var(--line); background: var(--panel); }
  .search { width: 420px; font: 13px var(--sans); padding: 5px 10px; border: 1px solid var(--line); border-radius: 4px; background: var(--bg); color: var(--fg); }
  .search:focus { outline: none; border-color: var(--accent); }
  .grow { flex: 1; }
  .tog { display: inline-flex; gap: 5px; align-items: center; font-size: 12px; color: var(--muted); cursor: pointer; }
  .toc { border-right: 1px solid var(--line); padding: 10px 6px; overflow: auto; display: flex; flex-direction: column; gap: 1px; }
  .toc button { all: unset; cursor: pointer; padding: 4px 10px; border-radius: 4px; }
  .toc button:hover { background: var(--hover); }
  .toc button:disabled { opacity: 0.4; cursor: default; }
  .toc .sub { padding-left: 22px; font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  main { overflow: auto; padding: 4px 28px 80px; }
  h2 { font-size: 16px; margin: 22px 0 8px; display: flex; gap: 8px; align-items: center; }
  h3 { font-size: 14px; margin: 18px 0 8px; font-family: var(--mono); }
  .set { padding: 8px 10px; margin: 0 -10px 4px; border-left: 2px solid transparent; }
  .set:hover { background: var(--hover); }
  .set.mod { border-left-color: var(--accent); }
  .title { display: flex; gap: 6px; align-items: center; margin-bottom: 2px; }
  .sec { color: var(--muted); }
  .key { margin-left: auto; font: 11px var(--mono); color: var(--muted); opacity: 0; }
  .set:hover .key { opacity: 1; }
  .reset { font-size: 11px; padding: 0 6px; }
  .help { color: var(--muted); font-size: 12.5px; margin: 0 0 6px; max-width: 720px; }
  .status { margin: 0 0 8px; }
  aside { border-left: 1px solid var(--line); min-height: 0; }
</style>
