import { describe, expect, it, vi } from "vitest";
import { type ChordRun, ChordWait, takesChords } from "./chord.svelte";

function press(init: KeyboardEventInit): KeyboardEvent {
  return new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
}

const ctrlK = { key: "k", code: "KeyK", ctrlKey: true };
const z = { key: "z", code: "KeyZ" };

/** Whether nothing past the wait sees the key: not the browser, and no other handler. */
function stopped(event: KeyboardEvent): boolean {
  return event.defaultPrevented && event.cancelBubble;
}

/** A chord that starts anywhere and runs `run`. */
const anywhere =
  (run: ChordRun = () => {}) =>
  () =>
    run;

/** A wait that Ctrl+K has started, whose chords run `run`. */
function waiting(run?: ChordRun): ChordWait {
  const wait = new ChordWait("other");
  wait.read(press(ctrlK), anywhere(run));
  return wait;
}

describe("ChordWait", () => {
  it("waits for the second key after Ctrl+K, and keeps Ctrl+K from anything else", () => {
    const wait = new ChordWait("other");
    const event = press(ctrlK);
    expect(wait.read(event, anywhere())).toBe(true);
    expect(wait.first).toBe("ctrl+k");
    expect(stopped(event)).toBe(true);
  });

  it("names Cmd+K as Ctrl+K on macOS", () => {
    const wait = new ChordWait("mac");
    wait.read(press({ key: "k", code: "KeyK", metaKey: true }), anywhere());
    expect(wait.first).toBe("ctrl+k");
  });

  it("asks where the chord starts with its first press and the press itself", () => {
    const start = vi.fn(() => null);
    const event = press(ctrlK);
    new ChordWait("other").read(event, start);
    expect(start).toHaveBeenCalledWith("ctrl+k", event);
  });

  it("runs a bound chord's action at the second key as told where it started, and the wait ends", () => {
    const run = vi.fn<ChordRun>();
    const wait = waiting(run);
    const event = press(z);
    expect(wait.read(event, () => null)).toBe(true);
    expect(run).toHaveBeenCalledExactlyOnceWith("toggle_detail_only", event);
    expect(wait.first).toBeNull();
    expect(stopped(event)).toBe(true);
  });

  it("reads the second key by where it is on a non-Latin layout", () => {
    const run = vi.fn<ChordRun>();
    waiting(run).read(press({ key: "з", code: "KeyP" }), anywhere());
    expect(run.mock.calls[0]?.[0]).toBe("pin");
  });

  it("ends at an unbound key or Esc, which do nothing else", () => {
    for (const init of [
      { key: "x", code: "KeyX" },
      { key: "ArrowDown", code: "ArrowDown" },
      { key: "Escape", code: "Escape" },
    ]) {
      const run = vi.fn<ChordRun>();
      const wait = waiting(run);
      const event = press(init);
      expect(wait.read(event, anywhere()), init.key).toBe(true);
      expect(wait.first, init.key).toBeNull();
      expect(stopped(event), init.key).toBe(true);
      expect(run, init.key).not.toHaveBeenCalled();
    }
  });

  it("ends at a key it can't name, like one with the Windows key", () => {
    const wait = waiting();
    const event = press({ key: "z", code: "KeyZ", metaKey: true });
    expect(wait.read(event, anywhere())).toBe(true);
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
      expect(wait.read(press(init), anywhere()), init.key).toBe(true);
      expect(wait.first, init.key).toBe("ctrl+k");
    }
  });

  it("ends when told to, as at a click", () => {
    const wait = waiting();
    wait.end();
    expect(wait.first).toBeNull();
  });

  it("leaves a key alone when no wait is on and it starts no chord, without asking where", () => {
    const wait = new ChordWait("other");
    const start = vi.fn(anywhere());
    const event = press(z);
    expect(wait.read(event, start)).toBe(false);
    expect(event.defaultPrevented || event.cancelBubble).toBe(false);
    expect(start).not.toHaveBeenCalled();
  });

  it("starts no wait where the page says no chord starts, as in a text box", () => {
    const wait = new ChordWait("other");
    const event = press(ctrlK);
    expect(wait.read(event, () => null)).toBe(false);
    expect(wait.first).toBeNull();
    expect(event.cancelBubble).toBe(false);
  });

  it("lets the focused part of the page take a chord's action, and runs it only when none does", () => {
    const pane = document.createElement("div");
    document.body.append(pane);
    const taken: string[] = [];
    const stop = takesChords((action) => {
      taken.push(action);
      return action === "toggle_detail_only";
    })(pane);
    const run = vi.fn<ChordRun>();
    const wait = new ChordWait("other");
    pane.addEventListener("keydown", (e) => {
      wait.read(e, anywhere(run));
    });
    for (const second of [z, { key: "o", code: "KeyO" }]) {
      pane.dispatchEvent(press(ctrlK));
      pane.dispatchEvent(press(second));
    }
    stop();
    pane.remove();

    expect(taken).toEqual(["toggle_detail_only", "toggle_files_mode"]);
    expect(run.mock.calls.map(([action]) => action)).toEqual(["toggle_files_mode"]);
  });
});
