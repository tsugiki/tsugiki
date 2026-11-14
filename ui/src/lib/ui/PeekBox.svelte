<!-- The Definition peek's box under its line, in the diff (#438) or in the file view (#457), with the edge
     that drags its height astride its bottom line (#458). It stays as wide as what it sits in, however far
     across the code scrolls, from the `--across` and `--width` its parent sets, and leaves the line numbers'
     `gutter` pixels on its left. Split, it fills the place below the code that `SplitPeek` gives it, which
     holds its edge, and lists its hits on the left (#459). The peek holds the focus itself while it has no
     list, as while a lookup from inside it runs, and takes it on a click on its header. While anything in it
     has the focus, `Alt+F12` looks up the word picked in its code (#447), and the window's keys never hear
     it. With no word picked there, it goes on to look up the word picked in the code under it. -->
<script lang="ts" module>
  /** The box's place: under its line, `top` pixels from the top of the rows, in `rows` rows, past the
   * line numbers' `gutter` pixels, or below the code, in the place `SplitPeek` gives it. */
  export type PeekPlace = { kind: "inline"; top: number; rows: number; gutter: number } | { kind: "split" };
</script>

<script lang="ts">
  import { PEEK_LEAST, PEEK_MOST } from "$lib/state/panes.svelte";
  import { peekSized, ROW, type Peek } from "$lib/state/peek";
  import type { PrFilesState } from "$lib/state/pr-files.svelte";
  import DefinitionPeek from "./DefinitionPeek.svelte";
  import { currentOs, keyName } from "./keys";
  import PaneEdge from "./PaneEdge.svelte";

  let {
    diff,
    peek,
    place,
  }: {
    diff: PrFilesState;
    peek: Peek;
    place: PeekPlace;
  } = $props();

  const os = currentOs();
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class={["peek sans", place.kind]}
  role="group"
  aria-label="Definition peek"
  tabindex="-1"
  style:top={place.kind === "inline" ? `${String(place.top)}px` : undefined}
  style:height={place.kind === "inline" ? `${String(place.rows * ROW)}px` : undefined}
  style:padding-left={place.kind === "inline" ? `${String(place.gutter)}px` : undefined}
  {@attach (box) => {
    // Go to definition with several hits hands the list the focus, for the user to pick one (#446). A lookup
    // from inside the peek has the peek hold it while it looks, and the list keeps it once the lookup
    // answers, unless the user moved it out of the peek (#447).
    const focus = diff.peekFocus;
    if (focus === undefined) return;
    diff.peekFocus = undefined;
    if (focus === "keep" && !box.contains(document.activeElement)) return;
    (box.querySelector<HTMLElement>("[role=listbox]") ?? box).focus({ preventScroll: true });
  }}
  onkeydown={(e) => {
    if (keyName(e, os) !== "alt+f12" || peek.code?.pick === undefined) return;
    e.preventDefault();
    e.stopPropagation();
    // A held key would ask the server again and again.
    if (!e.repeat) void diff.followPick();
  }}
>
  <DefinitionPeek
    {peek}
    split={place.kind === "split"}
    open={diff.openable}
    onclose={() => {
      diff.closePeek();
    }}
    onhit={(index) => {
      diff.showHit(index);
    }}
    onopen={() => {
      void diff.openHit();
    }}
    oncrumb={(index) => {
      diff.backTo(index);
    }}
  />
</div>
{#if place.kind === "inline" && peekSized(peek)}
  <!-- Astride the peek's bottom line, outside the peek, whose `overflow: hidden` would clip it. -->
  <div class="peek-edge" style:top={`${String(place.top + place.rows * ROW - 4)}px`}>
    <PaneEdge
      label="Definition peek height"
      sizes={{ kind: "box", value: diff.panes.peekRows, unit: ROW }}
      least={PEEK_LEAST}
      most={PEEK_MOST}
      onresize={(rows) => (diff.panes.peek = rows)}
      ondone={() => void diff.panes.save()}
      onreset={() => void diff.panes.reset("peek")}
    />
  </div>
{/if}

<style>
  /* The peek takes the places the layout gives it, and no more. Its lines are drawn inside it, so its header
     and the line under it keep their whole 20 px each. It reads as prose, as a review thread does. */
  .peek {
    box-sizing: border-box;
    overflow: hidden;
    background: color-mix(in srgb, currentColor 4%, transparent);
    box-shadow:
      inset 0 1px color-mix(in srgb, currentColor 15%, transparent),
      inset 0 -1px color-mix(in srgb, currentColor 15%, transparent);
  }

  .peek.inline {
    position: absolute;
    left: var(--across);
    width: var(--width);
  }

  /* Below the code, it fills the place `SplitPeek` gives it. */
  .peek.split {
    height: 100%;
  }

  /* The peek holds the focus for its parts, as while a lookup from inside it runs, and draws no outline. */
  .peek:focus {
    outline: none;
  }

  /* The strip the peek's bottom edge takes: 4 px of the peek and 4 px of the row under it, above a review
     thread there too. */
  .peek-edge {
    position: absolute;
    z-index: 1;
    left: var(--across);
    width: var(--width);
    height: 8px;
  }
</style>
