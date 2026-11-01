<script lang="ts">
  // PROTOTYPE — throwaway. Variant B, "Dialog": a preferences window over the app, a few hand-laid
  // pages instead of one list, and Apply / Cancel. Changes preview live; Cancel (or Esc) puts them back.
  // Repositories is a subtree in the left column, one page per Repository (as in A).
  import { settingDef, STATUS_PARTS, type SettingDef } from "$lib/core/settings";
  import { snapshotSettings, restoreSettings, settingsUi, repoConfig } from "$lib/state/settings.svelte";
  import { REPOS, say } from "$lib/state/app.svelte";
  import Field from "./Field.svelte";
  import Where from "./Where.svelte";
  import KeysTable from "./KeysTable.svelte";
  import RepoForm from "./RepoForm.svelte";
  import Files from "./Files.svelte";
  import Account from "./Account.svelte";
  import { dialog, DIALOG } from "../layout.svelte";
  let { onClose }: { onClose: () => void } = $props();

  const PAGES = ["General", "Account", "Repositories", "Sync", "Keyboard", "Advanced"] as const;
  type Page = (typeof PAGES)[number];
  const START: Record<string, Page> = { Account: "Account", Sync: "Sync", Review: "General", Appearance: "General", Storage: "Advanced", Keybindings: "Keyboard", Repositories: "Repositories", "Status bar": "General" };
  // svelte-ignore state_referenced_locally
  let page = $state<Page>(settingsUi.section?.startsWith("repo:") ? "Repositories" : START[settingsUi.section ?? ""] ?? "General");
  // svelte-ignore state_referenced_locally
  // The Repository whose page is open; null on the Repositories overview.
  let repo = $state<string | null>(settingsUi.section?.startsWith("repo:") ? settingsUi.section.slice(5) : null);
  const go = (p: Page, r: string | null = null) => { page = p; repo = r; files = false; };
  let files = $state(false);
  const before = snapshotSettings();
  const changed = $derived(JSON.stringify(snapshotSettings()) !== JSON.stringify(before));

  // Resizing: any edge or corner. It grows about its centre, so an edge follows the pointer and the size
  // changes by twice the delta. Drag the title bar to move it; double-click it or ⤢ to maximize.
  // Size and place last the session.
  let vw = $state(1500), vh = $state(950);
  const W = $derived(dialog.max ? vw - 32 : Math.min(dialog.w, vw - 32));
  const H = $derived(dialog.max ? vh - 32 : Math.min(dialog.h, vh - 32));
  function resize(dir: string) {
    return (e: PointerEvent) => {
      e.preventDefault();
      const el = e.currentTarget as HTMLElement;
      el.setPointerCapture(e.pointerId);
      const x0 = e.clientX, y0 = e.clientY, w0 = W, h0 = H;
      if (dialog.max) { dialog.max = false; dialog.x = dialog.y = 0; }
      const move = (m: PointerEvent) => {
        const dx = m.clientX - x0, dy = m.clientY - y0;
        if (dir.includes("e") || dir.includes("w")) dialog.w = Math.max(DIALOG.minW, Math.min(vw - 32, w0 + 2 * (dir.includes("e") ? dx : -dx)));
        if (dir.includes("s") || dir.includes("n")) dialog.h = Math.max(DIALOG.minH, Math.min(vh - 32, h0 + 2 * (dir.includes("s") ? dy : -dy)));
      };
      const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); };
      el.addEventListener("pointermove", move);
      el.addEventListener("pointerup", up);
    };
  }

  /** Drag by the title bar. The title bar never leaves the window, so the dialog can always be caught again. */
  function moveStart(e: PointerEvent) {
    if (dialog.max || (e.target as HTMLElement).closest("button") || e.button !== 0) return;
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    el.setPointerCapture(e.pointerId);
    const x0 = e.clientX - dialog.x, y0 = e.clientY - dialog.y;
    const move = (m: PointerEvent) => {
      const left = (vw - W) / 2, top = (vh - H) / 2;
      dialog.x = Math.max(80 - W - left, Math.min(vw - 80 - left, m.clientX - x0));
      dialog.y = Math.max(-top, Math.min(vh - 40 - top, m.clientY - y0));
    };
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  }
  const toggleMax = () => { dialog.max = !dialog.max; if (dialog.max) dialog.x = dialog.y = 0; };

  function cancel() { restoreSettings(before); onClose(); }
  function apply() { if (changed) say("Settings saved to config.toml and local.toml (prototype: in memory)"); onClose(); }
  const d = (k: string) => settingDef(k)!;
  const PAGE_FIELDS: Partial<Record<Page, { title: string; keys: string[] }[]>> = {
    General: [
      { title: "Appearance", keys: ["appearance.theme", "appearance.code_font_size", "appearance.list_key_hints"] },
      { title: "Status bar", keys: ["status_bar.visible", ...STATUS_PARTS] },
      { title: "Review", keys: ["review.diff_view", "review.collapse_generated"] },
    ],
    Sync: [{ title: "Refresh", keys: ["sync.workbench_minutes", "sync.open_item_seconds"] }, { title: "First sync", keys: ["sync.small_repository", "sync.first_sync_days"] }],
    Advanced: [{ title: "Editor", keys: ["review.editor_command"] }, { title: "Storage", keys: ["storage.config_file", "storage.data_dir"] }],
  };
</script>

<svelte:window bind:innerWidth={vw} bind:innerHeight={vh} onkeydown={(e) => { if (e.key === "Escape" && !e.defaultPrevented) { e.preventDefault(); cancel(); } }} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="scrim" onclick={cancel}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="dlg" role="dialog" aria-label="Settings" tabindex="-1" style="width: {W}px; height: {H}px; translate: {dialog.max ? 0 : dialog.x}px {dialog.max ? 0 : dialog.y}px" onclick={(e) => e.stopPropagation()}>
    {#each ["n", "s", "e", "w", "ne", "nw", "se", "sw"] as dir}<div class="rz {dir}" onpointerdown={resize(dir)} ondblclick={() => { dialog.w = DIALOG.w; dialog.h = DIALOG.h; dialog.x = dialog.y = 0; dialog.max = false; }} title="Drag to resize; double-click for the default size and place"></div>{/each}
    <header class:movable={!dialog.max} onpointerdown={moveStart} ondblclick={(e) => !(e.target as HTMLElement).closest("button") && toggleMax()}><b>Settings</b><span class="grow"></span>
      <button class="x" onclick={toggleMax} title={dialog.max ? "Restore size" : "Maximize (or double-click the title bar)"}>{dialog.max ? "❐" : "⤢"}</button>
      <button class="x" onclick={cancel} title="Cancel (Esc)">✕</button></header>
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
          <Account />
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
  .dlg { position: relative; background: var(--bg); border: 1px solid var(--line); border-radius: 8px; box-shadow: 0 12px 40px rgb(0 0 0 / 0.35); display: flex; flex-direction: column; overflow: hidden; font-size: 13px; }
  .rz { position: absolute; z-index: 2; }
  .rz.n, .rz.s { left: 8px; right: 8px; height: 6px; cursor: ns-resize; }
  .rz.e, .rz.w { top: 8px; bottom: 8px; width: 6px; cursor: ew-resize; }
  .rz.n { top: 0; } .rz.s { bottom: 0; } .rz.e { right: 0; } .rz.w { left: 0; }
  .rz.ne, .rz.nw, .rz.se, .rz.sw { width: 12px; height: 12px; }
  .rz.ne { top: 0; right: 0; cursor: nesw-resize; } .rz.sw { bottom: 0; left: 0; cursor: nesw-resize; }
  .rz.nw { top: 0; left: 0; cursor: nwse-resize; } .rz.se { bottom: 0; right: 0; cursor: nwse-resize; }
  header.movable { cursor: move; }
  header .x { cursor: pointer; }
  header { user-select: none; display: flex; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--line); }
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
