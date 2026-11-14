import { afterEach, describe, expect, it } from "vitest";
import { keepFocusIn } from "./focus";

/** A dialog in the page, with `inside` as its HTML, that keeps the focus in. */
function dialogOf(inside: string) {
  const dialog = document.createElement("div");
  dialog.tabIndex = -1;
  dialog.innerHTML = inside;
  dialog.addEventListener("keydown", keepFocusIn);
  document.body.append(dialog);
  return dialog;
}

/** Presses `key` on what has the focus, and says whether the browser may act on it. */
const press = (key: string, shiftKey = false) =>
  (document.activeElement ?? document.body).dispatchEvent(
    new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true }),
  );

afterEach(() => {
  document.body.innerHTML = "";
});

describe("keepFocusIn", () => {
  const CONTROLS = '<button>One</button><button disabled>Off</button><a href="#">Two</a><span>Text</span>';
  const named = (name: string) => document.activeElement?.textContent === name;

  it("takes Tab from the last control round to the first, and Shift+Tab back", () => {
    const dialog = dialogOf(CONTROLS);
    dialog.querySelector("a")?.focus();
    expect(press("Tab")).toBe(false);
    expect(named("One")).toBe(true);

    expect(press("Tab", true)).toBe(false);
    expect(named("Two")).toBe(true);
  });

  it("takes Shift+Tab from the dialog itself to its last control", () => {
    const dialog = dialogOf(CONTROLS);
    dialog.focus();
    expect(press("Tab", true)).toBe(false);
    expect(named("Two")).toBe(true);
  });

  it("leaves Tab between two controls inside, and every other key, to the browser", () => {
    const dialog = dialogOf(CONTROLS);
    dialog.querySelector("button")?.focus();
    expect(press("Tab")).toBe(true);
    expect(press("Enter")).toBe(true);
    dialog.querySelector("a")?.focus();
    expect(press("Tab", true)).toBe(true);
    expect(named("Two")).toBe(true);
  });

  it("keeps the focus on a dialog with no controls", () => {
    const dialog = dialogOf("<p>Nothing to press</p>");
    dialog.focus();
    expect(press("Tab")).toBe(false);
    expect(document.activeElement).toBe(dialog);
  });
});
