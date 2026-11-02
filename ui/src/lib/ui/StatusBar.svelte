<script lang="ts">
  import type { Status } from "$lib/state/status.svelte";

  let { status, error }: { status: Status | undefined; error?: string | undefined } = $props();
</script>

<footer role="status" class:offline={status?.state === "offline" || status?.state === "offline_as"}>
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
</style>
