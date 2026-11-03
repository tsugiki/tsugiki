<script lang="ts">
  import type { Status } from "$lib/state/status.svelte";

  let {
    status,
    error,
    visible = true,
    problems = [],
  }: {
    status: Status | undefined;
    error?: string | undefined;
    visible?: boolean;
    problems?: string[];
  } = $props();

  const offline = $derived(status?.state === "offline" || status?.state === "offline_as");
  // Hiding the bar can't hide these: nothing else would tell the user.
  const showStatus = $derived(visible || offline || status?.state === "problem" || Boolean(error));
  const listedProblems = $derived(problems.join(" · "));
</script>

<!-- Always in the page, because screen readers only announce changes in a live region that already exists. -->
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

<style>
  footer {
    position: fixed;
    inset: auto 0 0;
    padding: 2px 8px;
    font-size: 12px;
    background: #e8e8e8;
    color: #222;
  }

  .offline {
    background: #a85000;
    color: #fff;
  }

  .empty {
    padding: 0;
  }
</style>
