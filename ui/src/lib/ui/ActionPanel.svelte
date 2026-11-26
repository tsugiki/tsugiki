<script lang="ts">
  // The action panel (#1072): a small floating panel in the window's bottom-right corner that says what the
  // last action on an item did. It never takes the focus, and it is a polite live region that stays in the
  // page empty, so a screen reader announces each change as the status bar's status does.
  import { type ActionNotice, PANEL_EDGE, PANEL_GAP, overBox } from "$lib/state/action-notice.svelte";
  import { HUD_BOTTOM, type HudState } from "$lib/state/hud.svelte";

  let {
    notice,
    hud,
    statusBarHeight = 0,
    /** Says how much room a list that fills the window leaves at its end for the panel (#1072). */
    onclearance = () => {},
  }: {
    notice: ActionNotice;
    hud: HudState;
    statusBarHeight?: number;
    onclearance?: (clearance: number) => void;
  } = $props();

  /** How far the panel sits above the window's bottom edge: above the HUD while it shows, and otherwise above
   * the status bar at the height it draws, or at the edge itself while it draws nothing. */
  const bottom = $derived(
    hud.visible ? HUD_BOTTOM + hud.height + PANEL_GAP : statusBarHeight > 0 ? statusBarHeight + PANEL_GAP : 0,
  );

  /** The panel's painted size, which the browser reports as its text wraps and the window changes. It is the
   * panel's only layout measure, so no pointer move reads the layout. */
  let drawnWidth = $state(0);
  let drawnHeight = $state(0);
  let windowWidth = $state(window.innerWidth);
  let windowHeight = $state(window.innerHeight);

  /** Where the panel paints while it says something, so the countdown finds the pointer although the panel
   * itself takes no pointer event (#1072). */
  const box = $derived(
    notice.text === undefined || drawnHeight === 0
      ? undefined
      : {
          left: windowWidth - PANEL_EDGE - drawnWidth,
          right: windowWidth - PANEL_EDGE,
          top: windowHeight - bottom - drawnHeight,
          bottom: windowHeight - bottom,
        },
  );

  /** How much room a list that fills the window leaves at its end so the panel stays clear of its rows,
   * wherever it sits above the bar or the HUD. The window's own padding adds a little more. */
  const clearance = $derived(Math.max(0, bottom + drawnHeight - statusBarHeight));
  $effect(() => {
    onclearance(clearance);
  });

  /** Where the pointer was at its last move, which the cached box tests without reading the layout. */
  let point: { x: number; y: number } | undefined;
  /** Whether the pointer is on the panel, which holds the countdown. */
  let inside = false;

  /** Holds the countdown exactly while the pointer is on the panel, re-testing the cached box whenever the
   * text, the panel's place or its size changes under a pointer that stands still (#1072). */
  function settle() {
    const on = box !== undefined && point !== undefined && overBox(box, point.x, point.y);
    if (on === inside) return;
    inside = on;
    if (on) notice.hold();
    else notice.release();
  }
  // The box changes when the text, the HUD, the bar or the window does, so test it again then too.
  $effect(settle);

  function moved(event: PointerEvent) {
    point = { x: event.clientX, y: event.clientY };
    settle();
  }

  /** The window lost the pointer or the focus, so a countdown the pointer held runs again. */
  function leftWindow() {
    point = undefined;
    settle();
  }

  function pointedOut(event: PointerEvent) {
    if (event.relatedTarget !== null) return;
    leftWindow();
  }

  function resized() {
    windowWidth = window.innerWidth;
    windowHeight = window.innerHeight;
  }
</script>

<svelte:window onpointermove={moved} onpointerout={pointedOut} onblur={leftWindow} onresize={resized} />

<!-- Always in the page, so a screen reader announces changes in a region that already exists. It holds no
     focusable element, so the next key still goes to the list or the Detail pane. A log, not the status
     bar's `status`, so a test or a reader can tell the two apart. A click, the wheel and the focus pass
     through it, and the countdown finds the pointer by the box it paints. -->
<aside
  class="panel"
  class:shown={notice.text !== undefined}
  role="log"
  aria-live="polite"
  aria-label="What the action did"
  style:right={`${String(PANEL_EDGE)}px`}
  style:bottom={`${String(bottom)}px`}
  style:pointer-events="none"
  bind:offsetWidth={drawnWidth}
  bind:offsetHeight={drawnHeight}
>
  {notice.text ?? ""}
</aside>

<style>
  .panel {
    position: fixed;
    /* Over the panes and the rail's foot, which take at most 2, and under the palette, the dialogs and the
       HUD. */
    z-index: 3;
    /* No wider than the Detail pane's least width, so with the Detail pane open the panel stays off the Issue
       list. With no Detail pane, the list leaves the panel's room out of its end instead. */
    max-width: 300px;
    padding: 6px 10px;
    font-size: 12px;
    color: var(--fg);
    background: var(--bg-overlay);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 2px 8px var(--shadow);
    /* An empty panel draws nothing, while its live region stays in the page. */
    opacity: 0;
  }

  .shown {
    opacity: 1;
  }
</style>
