<script lang="ts">
  // PROTOTYPE — throwaway. Entry point: reads the URL and hands it to the app. Everything visible is in
  // $lib/ui, which reads $lib/state; core and data stay out of sight of this route.
  //   ?repo=owner/name (or home) picks the Repository; ?pr=N, ?issue=N, ?author=login, ?view=issues|archived|muted|deps, ?peek=owner/name/N.
  import { page } from "$app/state";
  import App from "$lib/ui/App.svelte";
  import { openRepo, REPOS } from "$lib/state/app.svelte";

  const p = page.url.searchParams;
  const num = (k: string) => +(p.get(k) ?? 0) || null;
  const repo = p.get("repo");
  if (repo === "home") openRepo(null);
  else if (repo && REPOS.includes(repo)) openRepo(repo);
</script>

<svelte:head><title>gh-rev — app shell PROTOTYPE</title></svelte:head>

<App start={num("pr")} startIssue={num("issue")} startAuthor={p.get("author")} startView={p.get("view")} startPeek={p.get("peek")} />
