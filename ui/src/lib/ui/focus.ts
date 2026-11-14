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
