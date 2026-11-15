import { afterEach, describe, expect, it } from "vitest";
import { focusBesideRail, keepFocusIn } from "./focus";

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

describe("focusBesideRail", () => {
  /** A PR view in the page, with `panel` as its tab's HTML, and the button that opens the rail. */
  function paneOf(panel: string, scrolls = false) {
    const pane = document.createElement("section");
    const attributes = scrolls ? ' tabindex="0" data-scrolls' : "";
    pane.innerHTML = `<button data-opens-rail>Files</button><div role="tabpanel"${attributes}>${panel}</div>`;
    document.body.append(pane);
    return pane;
  }
  const focusedName = () =>
    document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.textContent;

  it("gives the focus to the diff, or the file view over it, which leaves the diff inert", () => {
    const diff = '<div aria-label="Diff" tabindex="0" data-scrolls></div>';
    focusBesideRail(paneOf(diff));
    const toDiff = focusedName();
    document.body.innerHTML = "";
    const covered = `<div inert>${diff}</div><div aria-label="File view" tabindex="0" data-scrolls></div>`;

    focusBesideRail(paneOf(covered));

    expect([toDiff, focusedName()]).toEqual(["Diff", "File view"]);
  });

  it("gives the focus to the Conversation while it scrolls beside the rail, and else to the Files button", () => {
    const pane = paneOf("<p>The description</p>", true);
    focusBesideRail(pane);
    const beside = document.activeElement;
    document.body.innerHTML = "";

    focusBesideRail(paneOf("<p>The description</p>"));

    expect([beside, focusedName()]).toEqual([pane.querySelector('[role="tabpanel"]'), "Files"]);
  });

  it("passes over a region of the rail's own that scrolls, as before the diff has come", () => {
    const pane = paneOf("<p>Fetching the PR's commits…</p>");
    pane.insertAdjacentHTML(
      "beforeend",
      '<aside data-rail><div tabindex="0" data-scrolls>Tree</div></aside>',
    );

    focusBesideRail(pane);

    expect(focusedName()).toBe("Files");
  });

  it("does nothing without a PR view", () => {
    focusBesideRail(undefined);
    expect(document.activeElement).toBe(document.body);
  });
});
