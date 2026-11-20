import { afterEach, describe, expect, it, vi } from "vitest";
import { remapKeys } from "$lib/state/keymap.svelte";
import { keydown } from "$lib/testing/helpers";
import {
  actionLabel,
  chordName,
  currentOs,
  keyGuard,
  keyLabel,
  keyName,
  typesInTextBox,
  withKey,
} from "./keys";

describe("keyName", () => {
  it("names a key with its modifiers in one order", () => {
    expect(keyName(keydown({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+p",
    );
    expect(keyName(keydown({ key: "u", code: "KeyU", ctrlKey: true, altKey: true }), "other")).toBe(
      "ctrl+alt+u",
    );
    expect(keyName(keydown({ key: "F7", code: "F7", shiftKey: true }), "other")).toBe("shift+f7");
    expect(keyName(keydown({ key: "ArrowLeft", code: "ArrowLeft", altKey: true }), "other")).toBe(
      "alt+arrowleft",
    );
  });

  it("names a key on a Russian layout by where it is", () => {
    expect(keyName(keydown({ key: "з", code: "KeyP", ctrlKey: true }), "other")).toBe("ctrl+p");
    expect(keyName(keydown({ key: "З", code: "KeyP", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+p",
    );
    expect(keyName(keydown({ key: "б", code: "Comma", ctrlKey: true }), "other")).toBe("ctrl+,");
    expect(keyName(keydown({ key: "у", code: "KeyE" }), "other")).toBe("e");
  });

  it("names digits and punctuation on a non-Latin layout by where they are", () => {
    expect(keyName(keydown({ key: "ё", code: "Backquote" }), "other")).toBe("`");
    expect(keyName(keydown({ key: "ذ", code: "Digit0", ctrlKey: true }), "other")).toBe("ctrl+0");
  });

  it("names a shifted key it placed by what Shift types there on a US keyboard", () => {
    expect(keyName(keydown({ key: "Б", code: "Comma", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+<",
    );
    expect(keyName(keydown({ key: "!", code: "Digit1", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+!",
    );
  });

  it("names macOS's Option+Shift+/ as Windows names Alt+Shift+/", () => {
    const optionQuestion = { key: "¿", code: "Slash", altKey: true, shiftKey: true };
    expect([
      keyName(keydown(optionQuestion), "mac"),
      keyName(keydown({ ...optionQuestion, key: "?" }), "other"),
    ]).toEqual(["alt+?", "alt+?"]);
  });

  it("places a dead key by where it is", () => {
    expect(keyName(keydown({ key: "Dead", code: "KeyU", ctrlKey: true, altKey: true }), "other")).toBe(
      "ctrl+alt+u",
    );
    expect(keyName(keydown({ key: "Unidentified", code: "KeyP", ctrlKey: true }), "other")).toBe("ctrl+p");
  });

  it("keeps a Latin letter that isn't ASCII, which is that key on that layout", () => {
    expect(keyName(keydown({ key: "ß", code: "Minus", ctrlKey: true }), "other")).toBe("ctrl+ß");
    expect(keyName(keydown({ key: "ö", code: "Comma", ctrlKey: true }), "other")).toBe("ctrl+ö");
  });

  it("keeps the character when it can't place the key", () => {
    expect(keyName(keydown({ key: "Ж", code: "IntlBackslash" }), "other")).toBe("ж");
  });

  it("follows the layout on a Latin keyboard, as VS Code does", () => {
    expect(keyName(keydown({ key: "p", code: "KeyR", ctrlKey: true }), "other")).toBe("ctrl+p");
  });

  it("doesn't count Shift twice on a shifted symbol", () => {
    expect(keyName(keydown({ key: "?", code: "Slash", shiftKey: true }), "other")).toBe("?");
  });

  it("calls the space bar space, and keeps Shift on it", () => {
    expect(keyName(keydown({ key: " ", code: "Space", ctrlKey: true }), "other")).toBe("ctrl+space");
    expect(keyName(keydown({ key: " ", code: "Space", ctrlKey: true, shiftKey: true }), "other")).toBe(
      "ctrl+shift+space",
    );
  });

  it("has no name for a modifier on its own", () => {
    for (const key of ["Control", "Shift", "Alt", "Meta"]) {
      expect(keyName(keydown({ key, code: key }), "other")).toBeNull();
    }
  });

  it("uses Cmd where Ctrl is used elsewhere on macOS", () => {
    expect(keyName(keydown({ key: "p", code: "KeyP", metaKey: true }), "mac")).toBe("ctrl+p");
  });

  it("names a key by where it is when macOS's Option changes what it types", () => {
    const optionB = { key: "∫", code: "KeyB", metaKey: true, altKey: true };
    expect([
      keyName(keydown(optionB), "mac"),
      keyName(keydown({ ...optionB, metaKey: false }), "other"),
    ]).toEqual(["ctrl+alt+b", "alt+∫"]);
  });

  it("leaves macOS's own Ctrl keys alone, which move the caret in a text box", () => {
    expect(keyName(keydown({ key: "p", code: "KeyP", ctrlKey: true }), "mac")).toBeNull();
  });

  it("leaves the Windows key alone", () => {
    expect(keyName(keydown({ key: "p", code: "KeyP", metaKey: true }), "other")).toBeNull();
  });

  it("leaves AltGr alone, which types a character but reports Ctrl and Alt", () => {
    const altGr = keydown({ key: "€", code: "KeyU", ctrlKey: true, altKey: true });
    vi.spyOn(altGr, "getModifierState").mockImplementation((state) => state === "AltGraph");
    expect(keyName(altGr, "other")).toBeNull();
  });
});

describe("chordName", () => {
  it("joins a chord's two presses with a space", () => {
    expect(chordName("ctrl+k", "z")).toBe("ctrl+k z");
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

  it("draws an arrow key as its arrow", () => {
    expect([keyLabel("alt+arrowleft", "other"), keyLabel("alt+arrowright", "mac")]).toEqual([
      "Alt+←",
      "Option+→",
    ]);
  });

  it("names a chord's two presses, with Cmd on macOS", () => {
    expect([actionLabel("toggle_files_mode", "other"), actionLabel("toggle_detail_only", "mac")]).toEqual([
      "Ctrl+K O",
      "Cmd+K Z",
    ]);
  });

  it("names an action's key of one press", () => {
    expect(actionLabel("open_settings", "other")).toBe("Ctrl+,");
  });

  it("names Page Down and Page Up as keyboard.md does", () => {
    expect([keyLabel("alt+pagedown", "other"), keyLabel("ctrl+pageup", "mac")]).toEqual([
      "Alt+PageDown",
      "Cmd+PageUp",
    ]);
  });

  it("names a remapped key, and no key for an action left with none", () => {
    remapKeys({ toggle_files_mode: "ctrl+j shift+o", open_settings: "" });
    expect([actionLabel("toggle_files_mode", "other"), actionLabel("open_settings", "other")]).toEqual([
      "Ctrl+J Shift+O",
      null,
    ]);
    remapKeys({});
  });
});

describe("withKey", () => {
  it("puts the action's key after the text, or nothing while it has none", () => {
    expect(withKey("Settings", "open_settings", "other")).toBe("Settings (Ctrl+,)");
    remapKeys({ open_settings: "" });
    expect(withKey("Settings", "open_settings", "other")).toBe("Settings");
    remapKeys({});
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
    const event = keydown({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true });
    keyGuard({ command_palette: open }, "other")(event);
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
      { key: "q", code: "KeyQ" },
      { key: "Q", code: "KeyQ", shiftKey: true },
      { key: "?", code: "Slash", shiftKey: true },
    ];
    for (const init of owned) {
      const event = keydown(init);
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
      const event = keydown(init);
      keyGuard({}, "other")(event);
      expect(event.defaultPrevented, init.key).toBe(false);
    }
  });

  describe("in a text box", () => {
    function pressIn(box: HTMLElement, init: KeyboardEventInit): KeyboardEvent {
      const guard = keyGuard({}, "other");
      const event = keydown(init);
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

    it("lets a capital letter type, though Shift+Q is Tsugiki's elsewhere", () => {
      expect(typedIn("textarea", { key: "Q", code: "KeyQ", shiftKey: true }).defaultPrevented).toBe(false);
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

  it("owns F12, and leaves WebView2 the Ctrl+Shift+I that opens DevTools in a development build", () => {
    const f12 = keydown({ key: "F12", code: "F12" });
    const devtools = keydown({ key: "I", code: "KeyI", ctrlKey: true, shiftKey: true });

    keyGuard({}, "other")(f12);
    keyGuard({}, "other")(devtools);

    expect(f12.defaultPrevented).toBe(true);
    expect(devtools.defaultPrevented).toBe(false);
  });

  describe("with keys remapped", () => {
    afterEach(() => {
      remapKeys({});
    });

    it("runs a remapped action on its new key, and nothing on its old one", () => {
      remapKeys({ archive: "a" });
      const archive = vi.fn();
      const a = keydown({ key: "a", code: "KeyA" });
      const e = keydown({ key: "e", code: "KeyE" });

      keyGuard({ archive }, "other")(a);
      keyGuard({ archive }, "other")(e);

      expect(archive).toHaveBeenCalledOnce();
      expect([a.defaultPrevented, e.defaultPrevented]).toEqual([true, false]);
    });

    it("leaves the key of an action remapped to nothing to the browser", () => {
      remapKeys({ toggle_sidebar: "" });
      const ctrlB = keydown({ key: "b", code: "KeyB", ctrlKey: true });
      keyGuard({}, "other")(ctrlB);
      expect(ctrlB.defaultPrevented).toBe(false);
    });

    it("lets a plain key, Backspace and Delete type in a text box", () => {
      remapKeys({ archive: "a", mark_seen: "backspace", mark_unseen: "shift+delete" });
      const guard = keyGuard({}, "other");
      const box = document.createElement("input");
      document.body.append(box);
      const typed = [
        { key: "a", code: "KeyA" },
        { key: "Backspace", code: "Backspace" },
        { key: "Delete", code: "Delete", shiftKey: true },
      ].map((init) => {
        const event = keydown(init);
        box.addEventListener("keydown", guard, { once: true });
        box.dispatchEvent(event);
        return event.defaultPrevented;
      });
      box.remove();
      expect(typed).toEqual([false, false, false]);
    });

    it("lets Ctrl+Alt with a key that types type in a text box, unless its action works there", () => {
      remapKeys({
        archive: "ctrl+alt+q",
        toggle_sidebar: "ctrl+alt+s",
        command_palette: "ctrl+alt+p",
        quick_open: "ctrl+alt+o",
      });
      expect(typesInTextBox("ctrl+alt+p")).toBe(false);
      expect(typesInTextBox("ctrl+alt+o")).toBe(false);
      expect([
        typesInTextBox("ctrl+alt+q"),
        typesInTextBox("ctrl+alt+s"),
        typesInTextBox("ctrl+alt+u"),
      ]).toEqual([true, false, false]);
    });

    it("hands the bound action the press that ran it", () => {
      remapKeys({ archive: "a" });
      const archive = vi.fn();
      const a = keydown({ key: "a", code: "KeyA" });
      keyGuard({ archive }, "other")(a);
      expect(archive).toHaveBeenCalledWith(a);
    });
  });

  it("doesn't run an action for a key it can't name", () => {
    const open = vi.fn();
    keyGuard(
      { command_palette: open },
      "mac",
    )(keydown({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true }));
    expect(open).not.toHaveBeenCalled();
  });
});
