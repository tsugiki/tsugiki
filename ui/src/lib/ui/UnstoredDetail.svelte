<script lang="ts">
  // The Detail pane while Tsugiki fetches an Issue or a PR the database lacks (#981): `owner/repo #N`
  // loading, or why the fetch brought nothing, with Open in browser. Once the fetch finds the item, the pane
  // opens it.
  import { LinkOpener } from "$lib/state/link-opener.svelte";
  import type { FetchFailure, UnstoredViewState } from "$lib/state/unstored-view.svelte";

  let {
    view,
    focusable = false,
  }: {
    view: UnstoredViewState;
    /** Whether the pane takes the focus, as it does while the Issue list is hidden. */
    focusable?: boolean;
  } = $props();

  const links = new LinkOpener();
  const name = $derived(`${view.repository} #${String(view.number)}`);

  /** What the pane says of a fetch that brought nothing. */
  const words = (failure: FetchFailure) => {
    const number = `#${String(view.number)}`;
    if ("problem" in failure) return `Tsugiki couldn't fetch ${number}: ${failure.problem}`;
    switch (failure.why) {
      case "missing":
        return `GitHub has no Issue or PR ${number} in ${view.repository}.`;
      case "hidden":
        return `Your token can't see ${number} in ${view.repository}.`;
      case "offline":
        return `Tsugiki doesn't hold ${number}, and can't reach GitHub for it. The status bar says why.`;
      case "other":
        return `Tsugiki couldn't fetch ${number} from GitHub. tsugiki.log says why.`;
    }
  };
</script>

<section
  class="detail-pane"
  aria-label="Detail pane"
  tabindex={focusable ? -1 : undefined}
  data-pane="detail"
>
  <div class="page">
    <h2>{name}</h2>
    {#if view.failure === undefined}
      <p role="status" aria-busy="true">Fetching {name} from GitHub…</p>
    {:else}
      <p role="alert">{words(view.failure)}</p>
      {#if links.problem}
        <p role="alert">{links.problem}</p>
      {/if}
      <!-- GitHub sends an Issue's address on to the PR that has the number. -->
      <button onclick={() => void links.openOnGitHub(view.repository, { kind: "issue", number: view.number })}
        >Open in browser</button
      >
    {/if}
  </div>
</section>

<style>
  .page {
    max-width: 900px;
    padding: 10px 16px 24px;
  }

  h2 {
    margin: 4px 0 8px;
    font-size: 18px;
  }
</style>
