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

  /* GitHub's Primer colors, light. The window has no dark theme yet: one that sets `data-theme="dark"` on
     the root gets GitHub's dark colors below, as the diff's highlighting does. */
  .open {
    color: #1a7f37;
  }

  .done {
    color: #8250df;
  }

  .closed {
    color: #d1242f;
  }

  .muted {
    color: #59636e;
  }

  :global(:root[data-theme="dark"]) .open {
    color: #3fb950;
  }

  :global(:root[data-theme="dark"]) .done {
    color: #ab7df8;
  }

  :global(:root[data-theme="dark"]) .closed {
    color: #f85149;
  }

  :global(:root[data-theme="dark"]) .muted {
    color: #9198a1;
  }
</style>
