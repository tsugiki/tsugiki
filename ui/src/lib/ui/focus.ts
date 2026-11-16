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

/** The place of `box` in `scroller`, which scrolls it: its top in the scroller's content, which a scroll
 * leaves alone, and whether some of it, or all of it, shows. */
function placeIn(box: Element, scroller: Element) {
  const shown = scroller.getBoundingClientRect();
  const { top, bottom } = box.getBoundingClientRect();
  return {
    top: top - shown.top + scroller.scrollTop,
    some: bottom > shown.top && top < shown.bottom,
    all: top >= shown.top && bottom <= shown.bottom,
  };
}

/**
 * Before a change that can move `box` in `scroller`, as an answer from the core moves an item's row in the
 * Issue list (#796): what brings the box back on the screen once the change is drawn, given the box as it
 * is drawn then. If the change moved the box off the screen, the scroller scrolls as little as it takes, as
 * `scrollIntoView` does with `block: "nearest"`. A box the user had scrolled off the screen before stays off,
 * and the scroll stays put while the change leaves the box where it was.
 */
export function keepOnScreen(box: Element, scroller: Element): (moved: Element) => void {
  const before = placeIn(box, scroller);
  return (moved) => {
    const after = placeIn(moved, scroller);
    if (before.some && after.top !== before.top && !after.all) moved.scrollIntoView({ block: "nearest" });
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
