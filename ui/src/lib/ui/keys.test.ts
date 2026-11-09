import { describe, expect, it, vi } from "vitest";
import { currentOs, keyGuard, keyLabel, keyName, keysForBrowser } from "./keys";

function press(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
}

describe("keyName", () => {
  it("names a key with its modifiers in one order", () => {
    expect(keyName(press({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+p",
    );
    expect(keyName(press({ key: "u", code: "KeyU", ctrlKey: true, altKey: true }), "other")).toBe(
      "ctrl+alt+u",
    );
    expect(keyName(press({ key: "F7", code: "F7", shiftKey: true }), "other")).toBe("shift+f7");
    expect(keyName(press({ key: "ArrowLeft", code: "ArrowLeft", altKey: true }), "other")).toBe(
      "alt+arrowleft",
    );
  });

  it("names a key on a Russian layout by where it is", () => {
    expect(keyName(press({ key: "з", code: "KeyP", ctrlKey: true }), "other")).toBe("ctrl+p");
    expect(keyName(press({ key: "З", code: "KeyP", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+p",
    );
    expect(keyName(press({ key: "б", code: "Comma", ctrlKey: true }), "other")).toBe("ctrl+,");
    expect(keyName(press({ key: "у", code: "KeyE" }), "other")).toBe("e");
  });

  it("names digits and punctuation on a non-Latin layout by where they are", () => {
    expect(keyName(press({ key: "ё", code: "Backquote" }), "other")).toBe("`");
    expect(keyName(press({ key: "ذ", code: "Digit0", ctrlKey: true }), "other")).toBe("ctrl+0");
  });

  it("keeps Shift on a key it named by where it is", () => {
    expect(keyName(press({ key: "Б", code: "Comma", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+,",
    );
  });

  it("places a dead key by where it is", () => {
    expect(keyName(press({ key: "Dead", code: "KeyU", ctrlKey: true, altKey: true }), "other")).toBe(
      "ctrl+alt+u",
    );
    expect(keyName(press({ key: "Unidentified", code: "KeyP", ctrlKey: true }), "other")).toBe("ctrl+p");
  });

  it("keeps a Latin letter that isn't ASCII, which is that key on that layout", () => {
    expect(keyName(press({ key: "ß", code: "Minus", ctrlKey: true }), "other")).toBe("ctrl+ß");
    expect(keyName(press({ key: "ö", code: "Comma", ctrlKey: true }), "other")).toBe("ctrl+ö");
  });

  it("keeps the character when it can't place the key", () => {
    expect(keyName(press({ key: "Ж", code: "IntlBackslash" }), "other")).toBe("ж");
  });

  it("follows the layout on a Latin keyboard, as VS Code does", () => {
    expect(keyName(press({ key: "p", code: "KeyR", ctrlKey: true }), "other")).toBe("ctrl+p");
  });

  it("doesn't count Shift twice on a shifted symbol", () => {
    expect(keyName(press({ key: "?", code: "Slash", shiftKey: true }), "other")).toBe("?");
  });

  it("calls the space bar space", () => {
    expect(keyName(press({ key: " ", code: "Space", ctrlKey: true }), "other")).toBe("ctrl+space");
  });

  it("has no name for a modifier on its own", () => {
    for (const key of ["Control", "Shift", "Alt", "Meta"]) {
      expect(keyName(press({ key, code: key }), "other")).toBeNull();
    }
  });

  it("uses Cmd where Ctrl is used elsewhere on macOS", () => {
    expect(keyName(press({ key: "p", code: "KeyP", metaKey: true }), "mac")).toBe("ctrl+p");
  });

  it("names a key by where it is when macOS's Option changes what it types", () => {
    const optionB = { key: "∫", code: "KeyB", metaKey: true, altKey: true };
    expect([keyName(press(optionB), "mac"), keyName(press({ ...optionB, metaKey: false }), "other")]).toEqual(
      ["ctrl+alt+b", "alt+∫"],
    );
  });

  it("leaves macOS's own Ctrl keys alone, which move the caret in a text box", () => {
    expect(keyName(press({ key: "p", code: "KeyP", ctrlKey: true }), "mac")).toBeNull();
  });

  it("leaves the Windows key alone", () => {
    expect(keyName(press({ key: "p", code: "KeyP", metaKey: true }), "other")).toBeNull();
  });

  it("leaves AltGr alone, which types a character but reports Ctrl and Alt", () => {
    const altGr = press({ key: "€", code: "KeyU", ctrlKey: true, altKey: true });
    vi.spyOn(altGr, "getModifierState").mockImplementation((state) => state === "AltGraph");
    expect(keyName(altGr, "other")).toBeNull();
  });
});

describe("keyLabel", () => {
  it("names a key as the user reads it, with Cmd and Option on macOS", () => {
    expect([
      keyLabel("ctrl+alt+b", "other"),
      keyLabel("ctrl+alt+b", "mac"),
      keyLabel("f12", "other"),
    ]).toEqual(["Ctrl+Alt+B", "Cmd+Option+B", "F12"]);
  });
});

describe("currentOs", () => {
  it("tells macOS from the rest", () => {
    expect(currentOs("MacIntel")).toBe("mac");
    expect(currentOs("Win32")).toBe("other");
  });

  it("reads this browser's platform unless told otherwise", () => {
    vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    expect(currentOs()).toBe("mac");
    vi.restoreAllMocks();
  });
});

describe("keyGuard", () => {
  it("stops the browser and runs the action for a bound key", () => {
    const open = vi.fn();
    const event = press({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
    keyGuard({ "ctrl+shift+p": open }, "other")(event);
    expect(event.defaultPrevented).toBe(true);
    expect(open).toHaveBeenCalledOnce();
  });

  it("stops the browser for every key Tsugiki owns, bound or not", () => {
    const owned: KeyboardEventInit[] = [
      { key: "p", code: "KeyP", ctrlKey: true },
      { key: "з", code: "KeyP", ctrlKey: true },
      { key: "P", code: "KeyP", ctrlKey: true, shiftKey: true },
      { key: "H", code: "KeyH", ctrlKey: true, shiftKey: true },
      { key: "F12", code: "F12" },
      { key: "F12", code: "F12", altKey: true },
      { key: "F7", code: "F7" },
      { key: "F7", code: "F7", shiftKey: true },
      { key: "F8", code: "F8" },
      { key: "F8", code: "F8", shiftKey: true },
      { key: "f", code: "KeyF", ctrlKey: true },
      { key: "b", code: "KeyB", ctrlKey: true },
      { key: "u", code: "KeyU", ctrlKey: true, altKey: true },
      { key: "b", code: "KeyB", ctrlKey: true, altKey: true },
      { key: "Enter", code: "Enter", ctrlKey: true },
      { key: "E", code: "KeyE", ctrlKey: true, shiftKey: true },
      { key: "k", code: "KeyK", ctrlKey: true },
      { key: "ArrowLeft", code: "ArrowLeft", altKey: true },
      { key: "ArrowRight", code: "ArrowRight", altKey: true },
      { key: "G", code: "KeyG", ctrlKey: true, shiftKey: true },
      { key: "r", code: "KeyR", ctrlKey: true },
      { key: ",", code: "Comma", ctrlKey: true },
      { key: "=", code: "Equal", ctrlKey: true },
      { key: "-", code: "Minus", ctrlKey: true },
      { key: "0", code: "Digit0", ctrlKey: true },
      { key: "e", code: "KeyE" },
      { key: "m", code: "KeyM" },
      { key: "?", code: "Slash", shiftKey: true },
    ];
    for (const init of owned) {
      const event = press(init);
      keyGuard({}, "other")(event);
      expect(event.defaultPrevented, JSON.stringify(init)).toBe(true);
    }
  });

  it("leaves a key Tsugiki doesn't own to the browser", () => {
    for (const init of [
      { key: "c", code: "KeyC", ctrlKey: true },
      { key: "F5", code: "F5" },
      { key: "Escape", code: "Escape" },
      { key: "Control", code: "ControlLeft", ctrlKey: true },
    ]) {
      const event = press(init);
      keyGuard({}, "other")(event);
      expect(event.defaultPrevented, init.key).toBe(false);
    }
  });

  describe("in a text box", () => {
    function pressIn(box: HTMLElement, init: KeyboardEventInit): KeyboardEvent {
      const guard = keyGuard({}, "other");
      const event = press(init);
      document.body.append(box);
      document.addEventListener("keydown", guard, { capture: true });
      box.dispatchEvent(event);
      document.removeEventListener("keydown", guard, { capture: true });
      box.remove();
      return event;
    }

    function typedIn(tag: "input" | "textarea", init: KeyboardEventInit): KeyboardEvent {
      return pressIn(document.createElement(tag), init);
    }

    it("lets a plain key type", () => {
      expect(typedIn("input", { key: "e", code: "KeyE" }).defaultPrevented).toBe(false);
      expect(typedIn("textarea", { key: "?", code: "Slash", shiftKey: true }).defaultPrevented).toBe(false);
    });

    it("still owns a key with a modifier", () => {
      expect(typedIn("input", { key: "p", code: "KeyP", ctrlKey: true }).defaultPrevented).toBe(true);
    });

    it("lets Ctrl+Alt+B type, as AltGr sends it too", () => {
      const ctrlAltB = { key: "b", code: "KeyB", ctrlKey: true, altKey: true };
      expect(typedIn("input", ctrlAltB).defaultPrevented).toBe(false);
    });

    it("counts editable text as a text box", () => {
      const box = document.createElement("div");
      Object.defineProperty(box, "isContentEditable", { value: true });
      expect(pressIn(box, { key: "m", code: "KeyM" }).defaultPrevented).toBe(false);
    });
  });

  it("leaves F12 to DevTools in a development build, and owns it in a release", () => {
    const f12 = () => press({ key: "F12", code: "F12" });

    const development = f12();
    keyGuard({}, "other", keysForBrowser(true))(development);
    expect(development.defaultPrevented).toBe(false);

    const release = f12();
    keyGuard({}, "other", keysForBrowser(false))(release);
    expect(release.defaultPrevented).toBe(true);
  });

  it("doesn't run an action for a key it can't name", () => {
    const open = vi.fn();
    keyGuard(
      { "ctrl+shift+p": open },
      "mac",
    )(press({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true }));
    expect(open).not.toHaveBeenCalled();
  });
});
