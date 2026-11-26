// The helpers the UI's tests share that drive the DOM. Those that reach into the app's bindings and state
// live in state.ts, the data the e2e specs share too in fixtures.ts, and setup.ts fills jsdom's gaps before
// each test file.
import { fireEvent, screen, within } from "@testing-library/svelte";

/** Waits a macrotask, for what the code under test sets off with `setTimeout(…, 0)` or answers after a
 * promise chain longer than a few ticks. */
export const nextTask = () => new Promise((resolve) => setTimeout(resolve, 0));

/** A key pressed, as `init` says, that bubbles and can be cancelled, as the browser's do. */
export const keydown = (init: KeyboardEventInit) =>
  new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });

/** Presses `key` on the element that has the focus, as the user does. */
export const pressOnFocus = (key: string) =>
  fireEvent.keyDown(document.activeElement as HTMLElement, { key });

/** Clicks `element` with the caret `offset` characters into its text, where Chromium leaves a collapsed
 * selection after a click; jsdom lays nothing out to find the caret from. */
export const clickAt = async (element: Element, offset: number, init: MouseEventInit = {}) => {
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  let left = offset;
  let node = walker.nextNode() as Text | null;
  while (node !== null && left > node.length) {
    left -= node.length;
    node = walker.nextNode() as Text | null;
  }
  getSelection()?.collapse(node ?? element, node === null ? 0 : left);
  await fireEvent.click(element, init);
};

/** The Issue view's or the PR view's header buttons, by their words: Archive and its undo, Mute, Mute
 * author, then Mark Seen. */
export const headerButtons = () =>
  screen.getAllByRole("button", { name: /^(un)?(archive|mute|mark)/i }).map((button) => button.textContent);

/** Each segment's `aria-pressed` in the Files tab's switch, All files then One file. */
export const pressed = () => {
  const group = within(screen.getByRole("group", { name: "Files" }));
  return ["All files", "One file"].map((name) =>
    group.getByRole("button", { name }).getAttribute("aria-pressed"),
  );
};
