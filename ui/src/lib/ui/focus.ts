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

/** For a dialog's scrim, as its `onmousedown`: a press closes the dialog. The dialog goes and gives the focus
 * back before the browser acts on the press, which would then move the focus to the body, so it stops the
 * browser. */
export function closeOnPress(close: () => void) {
  return (e: MouseEvent) => {
    e.preventDefault();
    close();
  };
}
