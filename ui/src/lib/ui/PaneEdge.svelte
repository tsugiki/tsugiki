<!--
  The edge between two panes, which sizes the pane before it (#187), the edge inside the PR view's rail on
  its left, which sizes the rail (#603), the edge under the Definition peek, which sizes it in diff rows
  (#458), or the edge atop the split one, which sizes it in pixels (#459). A drag or an arrow key moves it,
  between the size's own minimum and maximum and the minimums of the panes beside it, and a double-click
  asks for the default size back.
-->
<script lang="ts" module>
  /** What an edge sizes. A pane's edge stands between panes side by side and sizes the pane on its left, in
   * CSS pixels, while the panes after it keep `nextLeast` between them. Its `value` is the pane's width, for
   * a screen reader, unless the pane takes half the room. An edge at the left of a pane, inside it, such as
   * the rail's, sizes that pane, `value` CSS pixels wide, while the pane before it keeps `previousLeast`, so
   * a drag left makes it wider. A box's edge lies under a box, such as the Definition peek, and sizes it in
   * steps `unit` pixels tall, of which it is `value` now. An edge above a box, such as the split Definition
   * peek, sizes the box under it, `value` CSS pixels tall now, so a drag up makes it taller. */
  export type Sizes =
    | { kind: "pane"; value: number | undefined; nextLeast: number }
    | { kind: "before"; value: number; previousLeast: number }
    | { kind: "box"; value: number; unit: number }
    | { kind: "above"; value: number };
</script>

<script lang="ts">
  import { onDestroy } from "svelte";
  import { currentOs, keyName } from "./keys";

  /** How far each arrow key moves the edge, in CSS pixels, or at least one step of a box's. */
  const STEP = 16;

  let {
    label,
    sizes,
    least,
    most,
    onresize,
    ondone,
    onreset,
  }: {
    label: string;
    sizes: Sizes;
    least: number;
    /** The most the size may get, even with room to spare. */
    most?: number;
    /** Gives what the edge sizes the size `size`, already kept between the minimums. */
    onresize: (size: number) => void;
    /** Says a drag or a held key moved the edge to where the user wants it, so the size can be saved. */
    ondone: () => void;
    onreset: () => void;
  } = $props();

  const vertical = $derived(sizes.kind === "pane" || sizes.kind === "before");
  const orientation = $derived(vertical ? "vertical" : "horizontal");
  /** The CSS pixels in one step of the size. */
  const unit = $derived(sizes.kind === "box" ? sizes.unit : 1);
  const ceiling = $derived(most ?? Infinity);
  /** The sign of the size's change as the edge moves down or right: an edge above its box gives it more room
   * as it moves up, and an edge before its pane as it moves left. */
  const grows = $derived(sizes.kind === "above" || sizes.kind === "before" ? -1 : 1);

  let edge = $state<HTMLElement>();
  /** Where the pointer was as the drag started, and the sizes that bounded it then. */
  let drag = $state<{ at: number; size: number; most: number }>();
  /** Whether the drag, or the arrow key held down, moved the edge, and so has a size to save. */
  let moved = false;

  /** The size the edge gives now, and the most it may take. Between panes, that's the width of the pane
   * before the edge, while the panes after keep their minimums: a pane after it with room to spare, such as
   * the Detail pane beyond the list, gives it up too. The other edges keep their widths, so they don't
   * count. At a pane's left, that's the width the pane keeps, which may be more than it shows while the
   * pane before it squeezes it, and it may grow by what the pane before it has above its minimum. A squeezed
   * pane has nothing to grow by, and keeps its width until the edge moves the other way. A box's edge has
   * `value` say its size. */
  function measure() {
    if (sizes.kind === "box" || sizes.kind === "above") {
      return { size: sizes.value, most: Math.max(least, ceiling) };
    }
    const here = edge as HTMLElement;
    if (sizes.kind === "before") {
      const pane = here.parentElement as HTMLElement;
      const shown = pane.getBoundingClientRect().width;
      const room = (pane.previousElementSibling as Element).getBoundingClientRect().width;
      const grown = Math.max(sizes.value, shown + room - sizes.previousLeast);
      return { size: sizes.value, most: Math.max(least, Math.min(ceiling, grown)) };
    }
    const nextLeast = sizes.nextLeast;
    const width = (here.previousElementSibling as Element).getBoundingClientRect().width;
    let after = 0;
    for (let next = here.nextElementSibling; next !== null; next = next.nextElementSibling) {
      if (next.getAttribute("role") !== "separator") after += next.getBoundingClientRect().width;
    }
    return { size: width, most: Math.max(least, Math.min(ceiling, width + after - nextLeast)) };
  }

  /** The pointer's place for event `e`, along the way the edge moves. */
  const along = (e: PointerEvent) => (vertical ? e.clientX : e.clientY);

  /** Moves the edge so it gives size `size`, within its limits. */
  function resize(size: number, most: number) {
    const next = Math.round(Math.min(most, Math.max(least, size)));
    if (next === sizes.value) return;
    moved = true;
    onresize(next);
  }

  /** Saves the size, unless the edge stayed where it was. */
  function done() {
    if (moved) ondone();
    moved = false;
  }

  // An edge that goes mid-drag or while an arrow is held, as the peek's does once its answer has only words
  // to say, hears no pointerup or keyup, so it saves where it got to as it goes.
  onDestroy(done);

  function onpointerdown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (edge as HTMLElement).setPointerCapture(e.pointerId);
    drag = { at: along(e), ...measure() };
  }

  function onpointermove(e: PointerEvent) {
    if (drag) resize(drag.size + (grows * (along(e) - drag.at)) / unit, drag.most);
  }

  function onpointerup() {
    drag = undefined;
    done();
  }

  /** The bare arrows across the edge move it, and leave Alt+← and the like to the window. A held arrow
   * saves once, as the user lets it go. The keys go no further, so they don't scroll the diff around the
   * peek or move the list's cursor too. */
  function onkeydown(e: KeyboardEvent) {
    const key = keyName(e, currentOs());
    const [back, forth] = vertical ? ["arrowleft", "arrowright"] : ["arrowup", "arrowdown"];
    const [less, more] = grows === 1 ? [back, forth] : [forth, back];
    if (key !== less && key !== more) return;
    e.preventDefault();
    e.stopPropagation();
    const { size, most } = measure();
    const step = Math.max(1, Math.round(STEP / unit));
    resize(size + (key === more ? step : -step), most);
  }
</script>

<!-- A separator that takes focus is a control, as a splitter is (WAI-ARIA's window splitter pattern). -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  bind:this={edge}
  class={["edge", orientation, { dragging: drag !== undefined }]}
  role="separator"
  aria-orientation={orientation}
  aria-label={label}
  aria-valuenow={sizes.value}
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

  /* Under a box, the edge fills the strip its parent places along the box's bottom. */
  .edge.horizontal {
    width: 100%;
    height: 100%;
    cursor: row-resize;
    background-size: 100% 1px;
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
