import type { RailTabId } from "$lib/state/rail.svelte";

/** For a dialog's first field, or a dialog with none, as an attachment: takes the focus, and gives it back
 * when the element goes. */
export function takeFocus(element: HTMLElement) {
  const previous = document.activeElement;
  element.focus();
  return () => {
    if (previous instanceof HTMLElement) {
      previous.focus();
    }
  };
}

/** Gives the focus to what the PR view's rail sits beside, in the PR view `pane` (#603): the diff on the
 * Files tab, or the file view over it, or the Conversation while it scrolls by itself beside the rail. With
 * neither, as once the rail has closed over the Conversation, which the whole pane scrolls again, or before
 * the diff has come, the focus goes to the button of the rail's `tab`, the one it shows or last showed. */
export function focusBesideRail(pane: Element | null | undefined, tab: RailTabId) {
  const scroller = scrollerIn(pane);
  // The rail's own scrollers come after the tab's, and don't count.
  const beside = scroller?.closest("[data-rail]") === null ? scroller : undefined;
  const button = pane?.querySelector<HTMLElement>(`[data-opens-rail="${tab}"]`);
  (beside ?? button)?.focus({ preventScroll: true });
}

/** The first region in `pane` that scrolls by itself, which `data-scrolls` marks, such as the diff or the
 * Conversation beside the rail. A region something covers, such as the diff under the file view, is inert,
 * and doesn't count. */
export function scrollerIn(pane: Element | null | undefined): HTMLElement | undefined {
  return pane?.querySelector<HTMLElement>("[data-scrolls]:not([inert] *)") ?? undefined;
}

/** The line along the top of the pane that holds the focus, which sticks there over what scrolls under it,
 * in CSS pixels. */
const FOCUS_LINE = 2;

/** The part of a box that must show for it to count as on the screen, in CSS pixels, so a layout that
 * rounds pixels its own way doesn't count. */
const SHOWS = 1;

/** The place of `box` in `scroller`, which scrolls it, below the focus line: whether more than `SHOWS` of
 * it shows, and how far its top lies above the part on show and its bottom below it. */
function placeIn(box: Element, scroller: Element) {
  const { top: shownTop, bottom: shownBottom } = scroller.getBoundingClientRect();
  const { top, bottom } = box.getBoundingClientRect();
  const from = shownTop + FOCUS_LINE;
  return {
    shows: Math.min(bottom, shownBottom) - Math.max(top, from) > SHOWS,
    above: from - top,
    below: bottom - shownBottom,
  };
}

/**
 * Before a change that can move `box` in `scroller`, as an answer from the core moves an item's row in the
 * Issue list (#796): what brings the box back once the change is drawn, given the box as it is drawn then.
 * If the box showed and the change took it off the screen, the scroller scrolls as little as it takes to
 * show all of it below the focus line. `scrollIntoView` would scroll the panes around the scroller too. A box
 * the user had scrolled off the screen stays off, and so does the scroll while some of the box still shows.
 */
export function keepOnScreen(box: Element, scroller: Element): (moved: Element) => void {
  const before = placeIn(box, scroller).shows;
  return (moved) => {
    const after = placeIn(moved, scroller);
    if (!before || after.shows) return;
    scroller.scrollTop += after.above > 0 ? -after.above : after.below;
  };
}

const CONTROLS = [
  "a[href]",
  "button:not(:disabled)",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

/** For a dialog, as its `onkeydown`: Tab and Shift+Tab go round the dialog's own controls, so the focus
 * can't leave it for the page behind. */
export function keepFocusIn(e: KeyboardEvent) {
  if (e.key !== "Tab") return;
  const dialog = e.currentTarget as HTMLElement;
  const controls = Array.from(dialog.querySelectorAll<HTMLElement>(CONTROLS));
  const [first] = controls;
  const last = controls.at(-1);
  if (first === undefined || last === undefined) {
    e.preventDefault();
  } else if (e.shiftKey && (document.activeElement === first || document.activeElement === dialog)) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}

/** For a dialog's scrim, as its `onmousedown`: a press closes the dialog. The dialog goes and gives the focus
 * back before the browser acts on the press, which would then move the focus to the body, so it stops the
 * browser. */
export function closeOnPress(close: () => void) {
  return (e: MouseEvent) => {
    e.preventDefault();
    close();
  };
}
