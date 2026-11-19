import { afterEach, describe, expect, it, vi } from "vitest";
import { commands } from "$lib/data/bindings";
import { tellCoreWhenShown } from "./window-shown";

vi.mock("$lib/data/bindings", () => ({
  commands: {
    windowLoaded: vi.fn(async () => null),
    windowHidden: vi.fn(async () => null),
    windowShown: vi.fn(async () => null),
  },
}));

/** Makes the window hidden or shown, as the browser says it, and tells the page it changed. */
function turn(hidden: boolean) {
  vi.spyOn(document, "hidden", "get").mockReturnValue(hidden);
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("tellCoreWhenShown", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("tells the core the window loaded, then each time it hides or shows", () => {
    const stop = tellCoreWhenShown();
    expect(commands.windowLoaded).toHaveBeenCalledOnce();
    expect(commands.windowShown).not.toHaveBeenCalled();

    turn(true);
    expect(commands.windowHidden).toHaveBeenCalledOnce();
    turn(false);

    expect(commands.windowShown).toHaveBeenCalledOnce();
    stop();
  });

  it("tells the core a window that loads hidden is hidden", () => {
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);

    const stop = tellCoreWhenShown();

    expect(commands.windowLoaded).toHaveBeenCalledOnce();
    expect(commands.windowHidden).toHaveBeenCalledOnce();
    stop();
  });

  it("shrugs off a core that can't hear it", async () => {
    vi.mocked(commands.windowLoaded).mockRejectedValueOnce(new Error("no core"));

    const stop = tellCoreWhenShown();
    await Promise.resolve();

    expect(commands.windowLoaded).toHaveBeenCalledOnce();
    stop();
  });

  it("tells the core nothing more once it stops", () => {
    const stop = tellCoreWhenShown();

    stop();
    turn(true);

    expect(commands.windowHidden).not.toHaveBeenCalled();
  });
});
