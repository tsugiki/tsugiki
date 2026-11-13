<!--
  A PR's state words, joined by a muted "·" (#600): a Review request in the accent colour, CI that failed and
  a Stale review in the danger colour, and CI that passed in the open state's green. A Stale review's hover
  lists the files changed since the Review. The dots are text, so the line reads the same to a screen reader
  and to a copy.
-->
<script lang="ts">
  import type { WorkbenchRow } from "$lib/state/workbench.svelte";
  import { prStateWords } from "./workbench-row";

  let { row }: { row: WorkbenchRow } = $props();

  // Svelte drops the spaces at the ends of an element's text, so the dot comes from here.
  const DOT = " · ";
</script>

<span class="words"
  >{#each prStateWords(row) as { words, tone, hover }, index (words)}{#if index > 0}<span class="dot"
        >{DOT}</span
      >{/if}<span class={tone} title={hover}>{words}</span>{/each}</span
>

<style>
  /* A narrow pane wraps the line between words, and never inside one. */
  .words {
    font-size: 11.5px;
  }

  .words > span {
    white-space: nowrap;
  }

  .dot {
    color: var(--fg-muted);
  }

  .request {
    font-weight: 600;
    color: var(--accent);
  }

  .bad {
    color: var(--danger);
  }

  .good {
    color: var(--state-open);
  }
</style>
