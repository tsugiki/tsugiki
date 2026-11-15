<!--
  The rail on the right of the PR view, beside the Conversation and Files tabs (#603). It holds one pane for
  now, Files, which the tree of the PR's changed files fills (#604). × in its corner closes it, and Esc in
  it hands the focus back to what it sits beside.
-->
<script lang="ts">
  import { currentOs, keyName } from "./keys";

  let {
    closeKey,
    onclose,
    onleave,
  }: {
    /** The key that closes the rail, for the tooltip of ×. */
    closeKey: string;
    onclose: () => void;
    /** Gives the focus back to the diff or the Conversation, as Esc in the rail does. */
    onleave: () => void;
  } = $props();

  /** Esc leaves the rail open and goes back to what it sits beside. It goes no further, so the window
   * doesn't also leave the PR for its Issue. */
  function onkeydown(e: KeyboardEvent) {
    if (keyName(e, currentOs()) !== "escape") return;
    e.preventDefault();
    e.stopPropagation();
    onleave();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="rail" aria-label="Rail" data-rail {onkeydown}>
  <header>
    <h3 class="small-heading">Files</h3>
    <button
      type="button"
      class="close"
      aria-label="Close the rail"
      title={`Close (${closeKey})`}
      onclick={onclose}>×</button
    >
  </header>
  <div class="pane" role="region" aria-label="Files"></div>
</aside>

<style>
  /* A step off the content, with a border, as the Sidebar is. The edge on its left measures the whole box,
     border too, and gives that width back. It gives way to the content in a narrow pane. */
  .rail {
    display: flex;
    flex: 0 1 var(--rail-width, 280px);
    flex-direction: column;
    box-sizing: border-box;
    min-width: 0;
    border-left: 1px solid var(--border);
    font-size: 13px;
    color: var(--fg-chrome);
    background: var(--bg-chrome);
  }

  header {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 4px 4px 10px;
  }

  h3 {
    flex: 1;
    margin: 0;
  }

  .close {
    all: unset;
    width: 22px;
    line-height: 22px;
    text-align: center;
    border-radius: 4px;
    cursor: pointer;
  }

  .close:hover {
    background: color-mix(in srgb, currentColor 12%, transparent);
  }

  .close:focus-visible {
    outline: 1px solid var(--accent);
  }

  .pane {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
</style>
