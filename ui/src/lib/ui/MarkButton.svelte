<!--
  The buttons in the Issue view's and the PR view's header (#622): "Mark Seen" while the item has New
  activity or is Marked unseen, and "Mark Unseen" otherwise, after the view's own buttons, such as the Issue
  view's Archive (#490).
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Seen } from "$lib/state/workbench.svelte";
  import { offersMarkSeen } from "./workbench-row";

  let { seen, onmark, children }: { seen: Seen; onmark: () => void; children?: Snippet } = $props();
</script>

<p class="actions">
  {@render children?.()}
  <button onclick={onmark}>{offersMarkSeen(seen) ? "Mark Seen" : "Mark Unseen"}</button>
</p>

<style>
  .actions {
    display: flex;
    gap: 6px;
    margin: 4px 0 8px;
  }

  .actions :global(button) {
    font-size: 12px;
  }
</style>
