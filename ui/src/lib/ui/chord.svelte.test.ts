import { describe, expect, it, vi } from "vitest";
import { ChordWait } from "./chord.svelte";

function press(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
}

const ctrlK = { key: "k", code: "KeyK", ctrlKey: true };
const z = { key: "z", code: "KeyZ" };

/** Whether nothing past the wait sees the key: not the browser, and no other handler. */
function stopped(event: KeyboardEvent): boolean {
  return event.defaultPrevented && event.cancelBubble;
}

/** A wait that Ctrl+K has started. */
function waiting(): ChordWait {
  const wait = new ChordWait("other");
  wait.read(press(ctrlK), true, {});
  return wait;
}

describe("ChordWait", () => {
  it("waits for the second key after Ctrl+K, and keeps Ctrl+K from anything else", () => {
    const wait = new ChordWait("other");
    const event = press(ctrlK);
    expect(wait.read(event, true, {})).toBe(true);
    expect(wait.first).toBe("ctrl+k");
    expect(stopped(event)).toBe(true);
  });

  it("names Cmd+K as Ctrl+K on macOS", () => {
    const wait = new ChordWait("mac");
    wait.read(press({ key: "k", code: "KeyK", metaKey: true }), true, {});
    expect(wait.first).toBe("ctrl+k");
  });

  it("runs a bound chord's action at the second key, and the wait ends", () => {
    const wait = waiting();
    const zoom = vi.fn();
    const event = press(z);
    expect(wait.read(event, true, { "ctrl+k z": zoom })).toBe(true);
    expect(zoom).toHaveBeenCalledOnce();
    expect(wait.first).toBeNull();
    expect(stopped(event)).toBe(true);
  });

  it("reads the second key by where it is on a non-Latin layout", () => {
    const wait = waiting();
    const pin = vi.fn();
    wait.read(press({ key: "з", code: "KeyP" }), true, { "ctrl+k p": pin });
    expect(pin).toHaveBeenCalledOnce();
  });

  it("ends at an unbound key or Esc, which do nothing else", () => {
    for (const init of [z, { key: "ArrowDown", code: "ArrowDown" }, { key: "Escape", code: "Escape" }]) {
      const wait = waiting();
      const pin = vi.fn();
      const event = press(init);
      expect(wait.read(event, true, { "ctrl+k p": pin }), init.key).toBe(true);
      expect(wait.first, init.key).toBeNull();
      expect(stopped(event), init.key).toBe(true);
      expect(pin, init.key).not.toHaveBeenCalled();
    }
  });

  it("ends at a key it can't name, like one with the Windows key", () => {
    const wait = waiting();
    const event = press({ key: "z", code: "KeyZ", metaKey: true });
    expect(wait.read(event, true, {})).toBe(true);
    expect(wait.first).toBeNull();
  });

  it("waits on through a modifier pressed alone, and through a held Ctrl+K", () => {
    const wait = waiting();
    for (const init of [
      { key: "Shift", code: "ShiftLeft", shiftKey: true },
      { key: "Control", code: "ControlLeft", ctrlKey: true },
      { key: "AltGraph", code: "AltRight" },
      { key: "CapsLock", code: "CapsLock" },
      { ...ctrlK, repeat: true },
    ]) {
      expect(wait.read(press(init), true, {}), init.key).toBe(true);
      expect(wait.first, init.key).toBe("ctrl+k");
    }
  });

  it("ends when told to, as at a click", () => {
    const wait = waiting();
    wait.end();
    expect(wait.first).toBeNull();
  });

  it("leaves a key alone when no wait is on and it starts none", () => {
    const wait = new ChordWait("other");
    const event = press(z);
    expect(wait.read(event, true, {})).toBe(false);
    expect(event.defaultPrevented || event.cancelBubble).toBe(false);
  });

  it("starts no wait where the caller says chords are off, as under a dialog", () => {
    const wait = new ChordWait("other");
    const event = press(ctrlK);
    expect(wait.read(event, false, {})).toBe(false);
    expect(wait.first).toBeNull();
    expect(event.cancelBubble).toBe(false);
  });

  it("starts no wait in a text box", () => {
    const wait = new ChordWait("other");
    const box = document.createElement("input");
    document.body.append(box);
    const event = press(ctrlK);
    let read: boolean | undefined;
    box.addEventListener("keydown", (e) => {
      read = wait.read(e, true, {});
    });
    box.dispatchEvent(event);
    box.remove();
    expect(read).toBe(false);
    expect(wait.first).toBeNull();
  });
});
