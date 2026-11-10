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
