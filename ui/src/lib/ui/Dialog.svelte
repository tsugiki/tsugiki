<!-- A modal dialog, or the command palette: a box on the scrim, which draws `children`. Esc and a press
     outside the box run `onclose`, Tab and Shift+Tab stay inside it, and it gives the focus back as it goes.
     window.css styles the box. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { closeOnPress, keepFocusIn, takeFocus } from "./focus";

  let {
    label,
    onclose,
    testid,
    size,
    flush = false,
    childTakesFocus = false,
    children,
  }: {
    /** The dialog's name, which a screen reader says. */
    label: string;
    /** Runs on Esc and on a press outside the box. */
    onclose: () => void;
    /** The scrim's test id is this with `-scrim` after it. */
    testid: string;
    /** `narrow` for a list that scrolls, `wide` for a form or the palette, `large` for pages beside their
     * list. */
    size: "narrow" | "wide" | "large";
    /** Whether the children reach the box's edges, as a text box across its top does. */
    flush?: boolean;
    /** Whether one of the children takes the focus as the dialog opens, as a text box does. Otherwise the
     * box takes it. */
    childTakesFocus?: boolean;
    children: Snippet;
  } = $props();

  function closeOnEscape(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window onkeydown={closeOnEscape} />

<!-- The keyboard's way out is Esc, on the window above. This is the mouse's. It goes by the press, because a
     drag that starts in a text box and ends out here also counts as a click out here. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="scrim" data-testid={`${testid}-scrim`} onmousedown={closeOnPress(onclose)}>
  <div
    class={["dialog", size, { flush }]}
    role="dialog"
    aria-modal="true"
    aria-label={label}
    tabindex="-1"
    {@attach (box) => (childTakesFocus ? undefined : takeFocus(box))}
    onkeydown={keepFocusIn}
    onmousedown={(e) => {
      e.stopPropagation();
    }}
  >
    {@render children()}
  </div>
</div>
