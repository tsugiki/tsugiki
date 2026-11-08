<!--
  The edge between two panes, which sizes the pane before it (#187). A drag or an arrow key moves it, between
  the pane's own minimum and maximum and the minimums of the panes after it, and a double-click asks for the
  pane's default width back.
-->
<script lang="ts">
  import { currentOs, keyName } from "./keys";

  /** How far each arrow key moves the edge, in CSS pixels. */
  const STEP = 16;

  let {
    label,
    value,
    least,
    most,
    nextLeast,
    onresize,
    ondone,
    onreset,
  }: {
    label: string;
    /** The width of the pane before the edge, for a screen reader, unless it takes half the room. */
    value: number | undefined;
    least: number;
    /** The widest the pane before the edge may get, even with room to spare. */
    most?: number;
    /** The least width the panes after the edge may take together. */
    nextLeast: number;
    /** Gives the pane before the edge width `width`, already kept between the minimums. */
    onresize: (width: number) => void;
    /** Says a drag or a held key moved the edge to where the user wants it, so the width can be saved. */
    ondone: () => void;
    onreset: () => void;
  } = $props();

  let edge = $state<HTMLElement>();
  /** Where the pointer was as the drag started, and the widths that sized the panes then. */
  let drag = $state<{ x: number; width: number; most: number }>();
  /** Whether the drag, or the arrow key held down, moved the edge, and so has a width to save. */
  let moved = false;

  /** The width of the pane before the edge now, and the most it may take while the panes after keep their
   * minimums. A pane after it with room to spare, such as the Detail pane beyond the list, gives it up too.
   * The other edges keep their widths, so they don't count. */
  function measure() {
    const here = edge as HTMLElement;
    const width = (here.previousElementSibling as Element).getBoundingClientRect().width;
    let after = 0;
    for (let next = here.nextElementSibling; next !== null; next = next.nextElementSibling) {
      if (next.getAttribute("role") !== "separator") after += next.getBoundingClientRect().width;
    }
    return { width, most: Math.max(least, Math.min(most ?? Infinity, width + after - nextLeast)) };
  }

  /** Moves the edge so the pane before it is `width` wide, within its limits. */
  function resize(width: number, most: number) {
    const next = Math.round(Math.min(most, Math.max(least, width)));
    if (next === value) return;
    moved = true;
    onresize(next);
  }

  /** Saves the width, unless the edge stayed where it was. */
  function done() {
    if (moved) ondone();
    moved = false;
  }

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (edge as HTMLElement).setPointerCapture(e.pointerId);
    drag = { x: e.clientX, ...measure() };
  }

  function onpointermove(e: PointerEvent) {
    if (drag) resize(drag.width + e.clientX - drag.x, drag.most);
  }

  function onpointerup() {
    drag = undefined;
    done();
  }

  /** The bare arrows move the edge, and leave Alt+← and the like to the window. A held arrow saves once,
   * as the user lets it go. */
  function onkeydown(e: KeyboardEvent) {
    const key = keyName(e, currentOs());
    if (key !== "arrowleft" && key !== "arrowright") return;
    e.preventDefault();
    const { width, most } = measure();
    resize(width + (key === "arrowright" ? STEP : -STEP), most);
  }
</script>

<!-- A separator that takes focus is a control, as a splitter is (WAI-ARIA's window splitter pattern). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={edge}
  class={["edge", { dragging: drag !== undefined }]}
  role="separator"
  aria-orientation="vertical"
  aria-label={label}
  aria-valuenow={value}
  aria-valuemin={least}
  aria-valuemax={most}
  tabindex="0"
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={onpointerup}
  ondblclick={onreset}
  {onkeydown}
  onkeyup={done}
></div>

<style>
  .edge {
    flex: none;
    width: 16px;
    cursor: col-resize;
    touch-action: none;
    background: linear-gradient(transparent, transparent) center / 1px 100% no-repeat;
  }

  .edge:hover,
  .edge:focus-visible,
  .edge.dragging {
    background-image: linear-gradient(
      color-mix(in srgb, currentColor 30%, transparent),
      color-mix(in srgb, currentColor 30%, transparent)
    );
  }
</style>
