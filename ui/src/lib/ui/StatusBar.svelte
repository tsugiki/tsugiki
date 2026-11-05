<script lang="ts">
  import type { LastSync, Status } from "$lib/state/status.svelte";
  import { syncedWords } from "./synced";

  const MINUTE = 60_000;

  let {
    status,
    error,
    visible = true,
    problems = [],
    lastSync = null,
    onrefresh = () => {},
  }: {
    status: Status | undefined;
    error?: string | undefined;
    visible?: boolean;
    problems?: string[];
    lastSync?: LastSync | null;
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
  const listedProblems = $derived(problems.join(" · "));
</script>

<!-- Always in the page, because screen readers only announce changes in a live region that already
     exists. -->
<footer
  role="status"
  class:offline
  class:empty={!showStatus && !listedProblems}
  class:beside-sync={lastSync !== null && showStatus}
>
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
<!-- Outside the live region, or a screen reader would read this out each minute. -->
{#if lastSync !== null && showStatus}
  <button class:offline onclick={onrefresh}>{syncedWords(lastSync.at, now)}</button>
{/if}

<style>
  footer {
    position: fixed;
    inset: auto 0 0;
    padding: 2px 8px;
    font-size: 12px;
    background: #e8e8e8;
    color: #222;
  }

  .beside-sync {
    padding-right: 11em;
  }

  button {
    position: fixed;
    right: 0;
    bottom: 0;
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
