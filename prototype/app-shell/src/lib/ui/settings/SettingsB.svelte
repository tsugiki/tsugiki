<script lang="ts">
  // PROTOTYPE — throwaway. Variant B, "Dialog": a preferences window over the app, a few hand-laid
  // pages instead of one list, and Apply / Cancel. Changes preview live; Cancel (or Esc) puts them back.
  // Repositories is a subtree in the left column, one page per Repository (as in A).
  import { settingDef, type SettingDef } from "$lib/core/settings";
  import { snapshotSettings, restoreSettings, settingsUi, repoConfig } from "$lib/state/settings.svelte";
  import { REPOS, wb, say } from "$lib/state/app.svelte";
  import Field from "./Field.svelte";
  import Where from "./Where.svelte";
  import KeysTable from "./KeysTable.svelte";
  import RepoForm from "./RepoForm.svelte";
  import Files from "./Files.svelte";
  let { onClose }: { onClose: () => void } = $props();

  const PAGES = ["General", "Account", "Repositories", "Sync", "Keyboard", "Advanced"] as const;
  type Page = (typeof PAGES)[number];
  const START: Record<string, Page> = { Account: "Account", Sync: "Sync", Review: "General", Appearance: "General", Storage: "Advanced", Keybindings: "Keyboard", Repositories: "Repositories" };
  // svelte-ignore state_referenced_locally
  let page = $state<Page>(settingsUi.section?.startsWith("repo:") ? "Repositories" : START[settingsUi.section ?? ""] ?? "General");
  // svelte-ignore state_referenced_locally
  // The Repository whose page is open; null on the Repositories overview.
  let repo = $state<string | null>(settingsUi.section?.startsWith("repo:") ? settingsUi.section.slice(5) : null);
  const go = (p: Page, r: string | null = null) => { page = p; repo = r; files = false; };
  let files = $state(false);
  const before = snapshotSettings();
  const changed = $derived(JSON.stringify(snapshotSettings()) !== JSON.stringify(before));

  function cancel() { restoreSettings(before); onClose(); }
  function apply() { if (changed) say("Settings saved to config.toml and local.toml (prototype: in memory)"); onClose(); }
  const d = (k: string) => settingDef(k)!;
  const PAGE_FIELDS: Partial<Record<Page, { title: string; keys: string[] }[]>> = {
    General: [{ title: "Appearance", keys: ["appearance.theme", "appearance.code_font_size"] }, { title: "Review", keys: ["review.diff_view", "review.collapse_generated"] }],
    Sync: [{ title: "Refresh", keys: ["sync.workbench_minutes", "sync.open_item_seconds"] }, { title: "First sync", keys: ["sync.small_repository", "sync.first_sync_days"] }],
    Advanced: [{ title: "Editor", keys: ["review.editor_command"] }, { title: "Storage", keys: ["storage.config_file", "storage.data_dir"] }],
  };
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); cancel(); } }} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={cancel}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="dlg" role="dialog" aria-label="Settings" tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header><b>Settings</b><span class="grow"></span><button class="x" onclick={cancel} title="Cancel (Esc)">✕</button></header>
    <div class="body">
      <nav>
        {#each PAGES as p}
          <button class:on={page === p && !repo && !files} onclick={() => go(p)}>{p}</button>
          {#if p === "Repositories"}
            {#each REPOS as r}<button class="sub" class:on={page === p && repo === r && !files} onclick={() => go(p, r)} title={r}>{r.split("/")[1]}</button>{/each}
          {/if}
        {/each}
      </nav>
      <section>
        {#if files}
          <div class="filesbox"><Files /></div>
        {:else if page === "Account"}
          <h3>GitHub</h3>
          <p class="status">✓ Signed in as <b>{wb.me}</b></p>
          {@render form([d("auth.source"), d("auth.token")], true)}
        {:else if page === "Repositories" && repo}
          <h3 class="mono">{repo}</h3>
          {#key repo}<RepoForm name={repo} />{/key}
        {:else if page === "Repositories"}
          <h3>Repositories</h3>
          <div class="repos">
            {#each REPOS as r}
              {@const c = repoConfig(r)}
              <button class="rrow" onclick={() => go("Repositories", r)}>
                <span class="mono">{r}</span>
                <span class="help">{c.filters.length} Filters · {c.mutedAuthors.length} muted authors{c.clone ? " · borrows from a clone" : ""}</span>
                <span class="go">›</span>
              </button>
            {/each}
          </div>
          <button class="add" onclick={() => say("Would ask for owner/name, then sync it (prototype)")}>+ Add repository…</button>
        {:else if page === "Keyboard"}
          <p class="help">VS Code's keys by default; on macOS Cmd stands for Ctrl. Click a key to change it. <Where where="synced" /></p>
          <KeysTable />
        {:else}
          {#each PAGE_FIELDS[page] ?? [] as group}
            <h3>{group.title}</h3>
            {@render form(group.keys.map(d), true)}
          {/each}
        {/if}
      </section>
    </div>
    <footer>
      <button class="link" onclick={() => (files = !files)}>{files ? "Back to the settings" : "Show the files…"}</button>
      <span class="grow"></span>
      <button onclick={cancel}>Cancel</button>
      <button class="primary" onclick={apply}>{changed ? "Apply" : "OK"}</button>
    </footer>
  </div>
</div>

{#snippet form(defs: SettingDef[], radios: boolean)}
  <div class="form">
    {#each defs as def (def.key)}
      <label class="lab" for="f-{def.key}">{def.type.kind === "bool" ? "" : def.label}</label>
      <div class="ctl"><Field {def} {radios} /> <Where where={def.where} /><div class="help">{def.help}</div></div>
    {/each}
  </div>
{/snippet}

<style>
  .scrim { position: fixed; inset: 0; background: rgb(0 0 0 / 0.35); z-index: 40; display: grid; place-items: center; }
  .dlg { width: min(880px, calc(100vw - 32px)); height: min(600px, calc(100vh - 32px)); background: var(--bg); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); display: flex; flex-direction: column; overflow: hidden; font-size: 13px; }
  header { display: flex; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--line); }
  .x { all: unset; cursor: pointer; padding: 0 6px; color: var(--muted); }
  .grow { flex: 1; }
  .body { flex: 1; display: grid; grid-template-columns: 170px 1fr; min-height: 0; }
  nav { background: var(--panel); border-right: 1px solid var(--line); padding: 8px; display: flex; flex-direction: column; gap: 2px; }
  nav button { all: unset; cursor: pointer; padding: 6px 10px; border-radius: 4px; }
  nav button:hover { background: var(--hover); }
  nav button.on { background: var(--sel); font-weight: 600; }
  section { overflow: auto; padding: 6px 20px 20px; min-height: 0; }
  h3 { font-size: 13px; margin: 16px 0 8px; padding-bottom: 4px; border-bottom: 1px solid var(--line); }
  .form { display: grid; grid-template-columns: 170px 1fr; gap: 10px 14px; }
  .lab { text-align: right; padding-top: 4px; }
  .help { color: var(--muted); font-size: 12px; margin-top: 3px; }
  .status { margin: 0 0 12px; color: var(--good); }
  nav .sub { padding: 4px 10px 4px 22px; font: 12px var(--mono); color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  nav .sub.on { color: var(--fg); }
  .mono { font-family: var(--mono); }
  .repos { border: 1px solid var(--line); border-radius: 6px; overflow: hidden; margin-bottom: 10px; }
  .rrow { all: unset; box-sizing: border-box; width: 100%; cursor: pointer; display: grid; grid-template-columns: 1fr auto; padding: 8px 12px; border-bottom: 1px solid var(--line); }
  .rrow:last-child { border-bottom: 0; }
  .rrow:hover { background: var(--hover); }
  .rrow .help { grid-column: 1; }
  .rrow .go { grid-column: 2; grid-row: 1 / 3; align-self: center; color: var(--muted); font-size: 18px; }
  .add { color: var(--accent); }
  .filesbox { height: 100%; min-height: 440px; padding-top: 10px; }
  footer { display: flex; gap: 8px; align-items: center; padding: 10px 14px; border-top: 1px solid var(--line); background: var(--panel); }
  .link { all: unset; cursor: pointer; color: var(--accent); font-size: 12px; }
  .primary { background: var(--accent); color: var(--on-accent); border-color: var(--accent); }
  .primary:hover { background: var(--accent); filter: brightness(1.1); }
</style>
