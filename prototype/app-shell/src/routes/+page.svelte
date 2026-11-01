<script lang="ts">
  // PROTOTYPE — throwaway. Entry point: reads the URL and hands it to the app. Everything visible is in
  // $lib/ui, which reads $lib/state; core and data stay out of sight of this route.
  //   ?repo=owner/name (or home) picks the Repository; ?pr=N, ?issue=N, ?author=login, ?view=issues|archived|muted|deps, ?peek=owner/name/N.
  //   ?settings (or ?settings=Keybindings, =repo:owner/name) opens Settings; ?variant=A|B|C picks its prototype variant.
  //   ?status=normal|syncing|jobs|offline|token|rate picks the simulated status bar state.
  import { page } from "$app/state";
  import App from "$lib/ui/App.svelte";
  import { replaceState, afterNavigate } from "$app/navigation";
  import { openRepo, REPOS } from "$lib/state/app.svelte";
  import { settingsUi, openSettings } from "$lib/state/settings.svelte";
  import { status, SCENARIOS } from "$lib/state/status.svelte";

  const p = page.url.searchParams;
  const num = (k: string) => +(p.get(k) ?? 0) || null;
  const repo = p.get("repo");
  if (repo === "home") openRepo(null);
  else if (repo && REPOS.includes(repo)) openRepo(repo);
  if (/^[ABC]$/.test(p.get("variant") ?? "")) settingsUi.variant = p.get("variant")!;
  if (p.has("settings")) openSettings(p.get("settings") || null);
  if (SCENARIOS.some(([k]) => k === p.get("status"))) status.scenario = p.get("status")!;

  // Keep ?variant= in the URL, so a reload shows the same variant. Not before the router is up.
  let ready = $state(false);
  afterNavigate(() => (ready = true));
  $effect(() => {
    const v = settingsUi.variant;
    if (!ready) return;
    const url = new URL(location.href);
    if (url.searchParams.get("variant") === v) return;
    url.searchParams.set("variant", v);
    replaceState(url, {});
  });
</script>

<svelte:head><title>gh-rev — app shell PROTOTYPE</title></svelte:head>

<App start={num("pr")} startIssue={num("issue")} startAuthor={p.get("author")} startView={p.get("view")} startPeek={p.get("peek")} />
