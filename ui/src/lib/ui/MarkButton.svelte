<!--
  The button in the Issue view's and the PR view's header (#622): "Mark Seen" while the item has New activity
  or is Marked unseen, and "Mark Unseen" otherwise.
-->
<script lang="ts">
  import type { Seen } from "$lib/state/workbench.svelte";
  import { offersMarkSeen } from "./workbench-row";

  let { seen, onmark }: { seen: Seen; onmark: () => void } = $props();
</script>

<p class="actions">
  <button onclick={onmark}>{offersMarkSeen(seen) ? "Mark Seen" : "Mark Unseen"}</button>
</p>

<style>
  .actions {
    margin: 4px 0 8px;
  }

  button {
    font-size: 12px;
  }
</style>
