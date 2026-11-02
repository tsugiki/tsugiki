<script lang="ts">
  import type { Status } from "$lib/state/status.svelte";

  let { status, error }: { status: Status | undefined; error?: string | undefined } = $props();
</script>

<footer role="status" class:offline={status?.connection === "offline"}>
  {#if error}
    Tsugiki can't read its status: {error}
  {:else if status?.problem}
    {status.problem.message}
  {:else if status?.connection === "offline"}
    Offline: read-only
  {:else if status?.login}
    {status.login}
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
