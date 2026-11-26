<script lang="ts">
  // The action panel (#1072): a small floating panel in the window's bottom-right corner that says what the
  // last action on an item did. It never takes the focus, and it is a polite live region that stays in the
  // page empty, so a screen reader announces each change as the status bar's status does.
  import { type ActionNotice, overPanel } from "$lib/state/action-notice.svelte";
  import { HUD_BOTTOM, type HudState } from "$lib/state/hud.svelte";

  let {
    notice,
    hud,
    statusBarHeight = 0,
  }: { notice: ActionNotice; hud: HudState; statusBarHeight?: number } = $props();

  /** The gap between the panel and the HUD, or the status bar, under it. */
  const GAP = 8;

  /** How far the panel sits above the window's bottom edge: above the HUD while it shows, and otherwise above
   * the status bar at the height it draws, or at the edge itself while it draws nothing. */
  const bottom = $derived(
    hud.visible ? HUD_BOTTOM + hud.height + GAP : statusBarHeight > 0 ? statusBarHeight + GAP : 0,
  );

  let panel = $state<HTMLElement>();
  /** Whether the pointer is over the panel, which holds its countdown. */
  let over = false;

  /** The panel takes no click, so a row under it still takes one; the window says where the pointer is, and
   * the pointer over the panel holds the countdown (#1072). */
  function pointerMoved(event: PointerEvent) {
    const inside = overPanel(panel, event);
    if (inside === over) return;
    over = inside;
    if (inside) notice.hold();
    else notice.release();
  }

  /** The pointer left the window, which lets a countdown it held run again. */
  function pointerLeft() {
    if (!over) return;
    over = false;
    notice.release();
  }
</script>

<svelte:window onpointermove={pointerMoved} onpointerleave={pointerLeft} />

<!-- Always in the page, so a screen reader announces changes in a region that already exists. It holds no
     focusable element, so the next key still goes to the list or the Detail pane. A log, not the status
     bar's `status`, so a test or a reader can tell the two apart. -->
<aside
  class="panel"
  class:shown={notice.text !== undefined}
  role="log"
  aria-live="polite"
  aria-label="What the action did"
  bind:this={panel}
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
    /* No wider than the Detail pane's least width, so the panel stays clear of the Issue list's rows. */
    max-width: 300px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--fg);
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 2px 8px var(--shadow);
    /* An empty panel takes no room and no click, while its live region stays in the page. It takes no click
       when it shows either: the pointer's place is asked of the window, so a row under it keeps its own. */
    opacity: 0;
    pointer-events: none;
  }

  .shown {
    opacity: 1;
  }
</style>
