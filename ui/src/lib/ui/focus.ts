/** For a dialog's first field, as an attachment: takes the focus, and gives it back when the field goes. */
export function takeFocus(input: HTMLInputElement) {
  const previous = document.activeElement;
  input.focus();
  return () => {
    if (previous instanceof HTMLElement) {
      previous.focus();
    }
  };
}
