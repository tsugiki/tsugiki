<script lang="ts">
  // PROTOTYPE — throwaway. Entry point: reads the URL and hands it to the app. Everything visible is in
  // $lib/ui, which reads $lib/state; core and data stay out of sight of this route.
  //   ?pr=N opens a PR, ?issue=N an Issue, ?author=login the temporary author Filter.
  import { page } from "$app/state";
  import App from "$lib/ui/App.svelte";

  const num = (k: string) => +(page.url.searchParams.get(k) ?? 0) || null;
</script>

<svelte:head><title>gh-rev — app shell PROTOTYPE</title></svelte:head>

<App start={num("pr")} startIssue={num("issue")} startAuthor={page.url.searchParams.get("author")} />
