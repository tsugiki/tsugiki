<script lang="ts">
  // The action panel (#1072): a small floating panel in the window's bottom-right corner that says what the
  // last action on an item did. It never takes the focus, and it is a polite live region that stays in the
  // page empty, so a screen reader announces each change as the status bar's status does.
  import { type ActionNotice, PANEL_GAP } from "$lib/state/action-notice.svelte";
  import { HUD_BOTTOM, type HudState } from "$lib/state/hud.svelte";

  let {
    notice,
    hud,
    statusBarHeight = 0,
    /** Says how tall the panel draws, so a list that fills the window can leave it the room. */
    onheight = () => {},
  }: {
    notice: ActionNotice;
    hud: HudState;
    statusBarHeight?: number;
    onheight?: (height: number) => void;
  } = $props();

  /** How far the panel sits above the window's bottom edge: above the HUD while it shows, and otherwise above
   * the status bar at the height it draws, or at the edge itself while it draws nothing. */
  const bottom = $derived(
    hud.visible ? HUD_BOTTOM + hud.height + PANEL_GAP : statusBarHeight > 0 ? statusBarHeight + PANEL_GAP : 0,
  );

  let drawn = $state(0);
  $effect(() => {
    onheight(drawn);
  });
</script>

<!-- Always in the page, so a screen reader announces changes in a region that already exists. It holds no
     focusable element, so the next key still goes to the list or the Detail pane. A log, not the status
     bar's `status`, so a test or a reader can tell the two apart. Once it shows, the panel's own pointer
     events hold its countdown, and the browser's hit test decides whether the pointer is on it. -->
<aside
  class="panel"
  class:shown={notice.text !== undefined}
  role="log"
  aria-live="polite"
  aria-label="What the action did"
  onpointerenter={() => notice.hold()}
  onpointerleave={() => notice.release()}
  bind:offsetHeight={drawn}
  style:bottom={`${String(bottom)}px`}
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
    /* No wider than the Detail pane's least width, so with the Detail pane open the panel stays off the Issue
       list. With no Detail pane, the list leaves the panel's height out of its room instead. */
    max-width: 300px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--fg);
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 2px 8px var(--shadow);
    /* An empty panel takes no click and no room, while its live region stays in the page. Once it shows it
       takes the pointer, so the pointer on it holds its countdown. */
    opacity: 0;
    pointer-events: none;
  }

  .shown {
    opacity: 1;
    pointer-events: auto;
  }
</style>
