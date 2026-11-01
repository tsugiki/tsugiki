<script lang="ts">
  // PROTOTYPE — throwaway. The app: the Home page, or the open Repository's Workspace. Switching
  // Repository remounts the Workspace, so selection and scope start fresh there.
  import { app, toast } from "$lib/state/app.svelte";
  import "./theme.css";
  import Home from "./Home.svelte";
  import Workspace from "./Workspace.svelte";
  import Settings from "./settings/Settings.svelte";
  import { settingsUi, value } from "$lib/state/settings.svelte";
  import { status, SCENARIOS, critical } from "$lib/state/status.svelte";
  import StatusBar from "./StatusBar.svelte";
  import VariantBar from "./settings/VariantBar.svelte";

  let { start = null, startIssue = null, startAuthor = null, startView = null, startPeek = null }: {
    start?: number | null; startIssue?: number | null; startAuthor?: string | null; startView?: string | null; startPeek?: string | null;
  } = $props();

  let showToast = $state(false);
  $effect(() => {
    if (!toast.at) return;
    showToast = true;
    const t = setTimeout(() => (showToast = false), 3500);
    return () => clearTimeout(t);
  });
  // Settings that change the look apply at once: theme and code font size.
  $effect(() => {
    const root = document.documentElement;
    const theme = value("appearance.theme");
    if (theme === "system") delete root.dataset.theme; else root.dataset.theme = String(theme);
    root.style.setProperty("--code-size", `${value("appearance.code_font_size")}px`);
  });
  // The status bar can be switched off; a problem brings it back by itself.
  const bar = $derived(value("status_bar.visible") === true || critical());
  $effect(() => document.documentElement.style.setProperty("--status-h", bar ? "22px" : "0px"));
  // The URL's start-up selection applies to the first Repository shown only.
  let first = $state(true);
  $effect(() => { app.repo; return () => (first = false); });
</script>

<div class="main">
  {#if app.repo == null}<Home />
  {:else}
    {#key app.repo}
      <Workspace start={first ? start : null} startIssue={first ? startIssue : null} startAuthor={first ? startAuthor : null} startView={first ? startView : null} startPeek={first ? startPeek : null} />
    {/key}
  {/if}
</div>
{#if bar}<StatusBar />{/if}
<!-- PROTOTYPE: the status bar's states that need a backend, simulated. -->
{#if !settingsUi.open}<VariantBar variants={SCENARIOS} current={status.scenario} onChange={(v) => (status.scenario = v)} bottom={40} prefix="Status" />{/if}
{#if settingsUi.open}<Settings />{/if}
{#if showToast}<div class="toast">{toast.text}</div>{/if}

<style>
  .main { height: calc(100vh - var(--status-h)); overflow: auto; }
  .toast { position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%); background: var(--fg); color: var(--bg); padding: 8px 14px; border-radius: 6px; font-size: 13px; z-index: 90; max-width: 70vw; }
</style>
