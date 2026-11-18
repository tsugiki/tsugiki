<!--
  The buttons in the Issue view's and the PR view's header (#622): "Mark Seen" while the item has New
  activity or is Marked unseen, and "Mark Unseen" otherwise, after the view's own buttons, such as the Issue
  view's Archive (#490). A Muted item takes neither mark, so it shows only the view's own buttons (#492).
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Seen } from "$lib/state/workbench.svelte";
  import { offersMarkSeen } from "./workbench-row";

  let {
    seen,
    onmark,
    marks = true,
    children,
  }: { seen: Seen; onmark: () => void; marks?: boolean; children?: Snippet } = $props();
</script>

<p class="actions">
  {@render children?.()}
  {#if marks}
    <button onclick={onmark}>{offersMarkSeen(seen) ? "Mark Seen" : "Mark Unseen"}</button>
  {/if}
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
