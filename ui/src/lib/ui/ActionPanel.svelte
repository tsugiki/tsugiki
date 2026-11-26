<script lang="ts">
  // The action panel (#1072): a small floating panel in the window's bottom-right corner that says what the
  // last action on an item did. It never takes the focus, and it is a polite live region that stays in the
  // page empty, so a screen reader announces each change as the status bar's status does.
  import type { ActionNotice } from "$lib/state/action-notice.svelte";
  import type { HudState } from "$lib/state/hud.svelte";

  let {
    notice,
    hud,
    statusBarVisible = true,
  }: { notice: ActionNotice; hud: HudState; statusBarVisible?: boolean } = $props();

  /** How far the HUD's panel sits above the window's bottom edge, as its own stylesheet sets it. */
  const ABOVE_BAR = 32;
  /** The gap between the panel and the HUD when both show. */
  const GAP = 8;

  /** How far the panel sits above the window's bottom edge: above the HUD while it shows, and otherwise
   * above the status bar, or at the edge itself while the bar is hidden. */
  const bottom = $derived(hud.visible ? ABOVE_BAR + hud.height + GAP : statusBarVisible ? ABOVE_BAR : 0);
</script>

<!-- Always in the page, so a screen reader announces changes in a region that already exists. It holds no
     focusable element, so the next key still goes to the list or the Detail pane. A log, not the status
     bar's `status`, so a test or a reader can tell the two apart. -->
<aside
  class="panel"
  class:shown={notice.text !== undefined}
  role="log"
  aria-live="polite"
  aria-label="What the action did"
  style:bottom={`${String(bottom)}px`}
  onmouseenter={() => notice.hold()}
  onmouseleave={() => notice.release()}
>
  {notice.text ?? ""}
</aside>

<style>
  .panel {
    position: fixed;
    /* Over the panes and the rail's foot, which take at most 2, and under the palette, the dialogs and the
       HUD. */
    z-index: 3;
    right: 8px;
    /* No wider than the Detail pane's least width, so the panel stays clear of the Issue list's rows. */
    max-width: 300px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--fg);
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 2px 8px var(--shadow);
    /* An empty panel takes no room and no click, while its live region stays in the page. */
    opacity: 0;
    pointer-events: none;
  }

  .shown {
    opacity: 1;
    pointer-events: auto;
  }
</style>
