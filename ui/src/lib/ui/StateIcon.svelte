<!--
  An Issue's or a PR's Octicon in GitHub's color for its state (#188). Its tooltip names the state, which is
  its name for a screen reader too: a heading it sits in says the state before the title. It adds no text,
  so the text of a line it sits in stays the same.
-->
<script lang="ts">
  import type { Octicon } from "./octicons";

  let { icon }: { icon: Octicon } = $props();
</script>

<span class={["octicon", icon.tone]} role="img" title={icon.label}>
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    {#each icon.paths as path (path)}
      <path d={path} />
    {/each}
  </svg>
</span>

<style>
  /* Inline, so a title that wraps keeps the icon on its first line. */
  .octicon {
    display: inline-block;
    line-height: 0;
    vertical-align: text-bottom;
  }

  svg {
    fill: currentColor;
  }

  /* GitHub's state colours, from the theme in force (#515). */
  .open {
    color: var(--state-open);
  }

  .done {
    color: var(--state-done);
  }

  .closed {
    color: var(--state-closed);
  }

  .muted {
    color: var(--state-muted);
  }
</style>
