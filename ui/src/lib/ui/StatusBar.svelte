<script lang="ts">
  import type { Cloning, LastSync, Status, SyncActivity } from "$lib/state/status.svelte";
  import { cloningWords, elsewhereWords, pausedWords, syncedWords, syncingWords } from "./synced";

  const MINUTE = 60_000;

  let {
    status,
    error,
    visible = true,
    problems = [],
    lastSync = null,
    syncActivity = null,
    cloning = null,
    onrefresh = () => {},
  }: {
    status: Status | undefined;
    error?: string | undefined;
    visible?: boolean;
    problems?: string[];
    lastSync?: LastSync | null;
    syncActivity?: SyncActivity | null;
    cloning?: Cloning | null;
    onrefresh?: () => void;
  } = $props();

  // "Synced 2 min ago" has to move on by itself between syncs.
  let now = $state(new Date());
  $effect(() => {
    const ticking = setInterval(() => {
      now = new Date();
    }, MINUTE);
    return () => {
      clearInterval(ticking);
    };
  });

  const offline = $derived(status?.state === "offline" || status?.state === "offline_as");
  // Hiding the bar can't hide these: nothing else would tell the user.
  const showStatus = $derived(visible || offline || status?.state === "problem" || Boolean(error));
  const paused = $derived(pausedWords(syncActivity));
  const listedProblems = $derived([...problems, ...(paused ? [paused] : [])].join(" · "));
  // A sync of the Repository open before the window switched says nothing about the one open now.
  const syncing = $derived(
    syncActivity?.open?.repository === lastSync?.repository ? syncingWords(syncActivity) : null,
  );
  const elsewhere = $derived(elsewhereWords(syncActivity));
  const clone = $derived(cloningWords(cloning));
  const syncWords = $derived(
    [syncing ?? syncedWords(lastSync?.at ?? null, now), clone, elsewhere]
      .filter((words) => words !== null)
      .join(" · "),
  );
</script>

<!-- One row, so the sync words take the room they need and the status gets the rest. -->
<div class="bar">
  <!-- Always in the page, because screen readers only announce changes in a live region that already
       exists. -->
  <footer role="status" class:offline class:empty={!showStatus && !listedProblems}>
    {#if showStatus}
      {#if error}
        Tsugiki can't read its status: {error}
      {:else if status?.state === "problem"}
        {status.problem.message}
      {:else if status?.state === "offline"}
        Offline: read-only
      {:else if status?.state === "offline_as"}
        {`${status.last_login} · Offline: read-only`}
      {:else if status?.state === "online"}
        {status.login}
      {:else if status?.state === "connecting_as"}
        {`${status.last_login} · Connecting…`}
      {:else}
        Connecting…
      {/if}
    {/if}
    <span>{showStatus && listedProblems ? `· ${listedProblems}` : listedProblems}</span>
  </footer>
  <!-- Outside the live region, or a screen reader would read this out each minute, and each page of a
       sync. -->
  {#if lastSync !== null && showStatus}
    <button class:offline onclick={onrefresh}>{syncWords}</button>
  {/if}
</div>

<style>
  .bar {
    position: fixed;
    inset: auto 0 0;
    display: flex;
  }

  footer {
    flex: 1;
    min-width: 0;
    padding: 2px 8px;
    font-size: 12px;
    background: #e8e8e8;
    color: #222;
  }

  button {
    flex: none;
    align-self: flex-end;
    white-space: nowrap;
    padding: 2px 8px;
    border: none;
    font-size: 12px;
    background: #e8e8e8;
    color: #222;
    cursor: pointer;
  }

  .offline {
    background: #a85000;
    color: #fff;
  }

  .empty {
    padding: 0;
  }
</style>
