import { describe, expect, it, vi } from "vitest";
import type { Miss } from "$lib/data/bindings";
import { browserClock, HudState } from "./hud.svelte";

/** A clock the test moves by hand, and frames that come only when the test draws one. */
function harness() {
  let now = 0;
  const waiting = new Map<number, (time: number) => void>();
  let handles = 0;
  const clock = {
    now: () => now,
    requestFrame: vi.fn((callback: (time: number) => void) => {
      handles += 1;
      waiting.set(handles, callback);
      return handles;
    }),
    cancelFrame: vi.fn((handle: number) => {
      waiting.delete(handle);
    }),
    afterPaint: vi.fn((callback: () => void) => {
      callback();
    }),
  };
  const core = {
    timeRunning: vi.fn(() => Promise.resolve<number | null>(640)),
    recordMeasurement: vi.fn(() => Promise.resolve("the line")),
    recordMiss: vi.fn<(miss: Miss) => Promise<string>>(() => Promise.resolve("the line")),
  };
  const hud = new HudState(clock, core);
  /** Draws a frame at `time`, if the HUD asked for one. */
  const frame = (time: number) => {
    now = time;
    const [handle, callback] = [...waiting][0] ?? [];
    if (handle === undefined || callback === undefined) return;
    waiting.delete(handle);
    callback(time);
  };
  /** Draws frames `every` ms apart up to `until`, after the last one drawn. */
  const frames = (from: number, until: number, every: number) => {
    for (let time = from; time <= until; time += every) frame(time);
  };
  return {
    hud,
    clock,
    core,
    frame,
    frames,
    waiting,
    setNow: (time: number) => {
      now = time;
    },
  };
}

describe("showing and hiding", () => {
  it("starts hidden, and asks for no frame until it is shown", () => {
    const { hud, clock } = harness();
    expect(hud.visible).toBe(false);
    expect(clock.requestFrame).not.toHaveBeenCalled();
  });

  it("asks for frames while it shows, and stops when hidden", () => {
    const { hud, frames, waiting } = harness();
    hud.toggle();
    expect(hud.visible).toBe(true);
    frames(0, 100, 16);
    expect(waiting.size).toBe(1);
    hud.toggle();
    expect(hud.visible).toBe(false);
    expect(waiting.size).toBe(0);
  });
});

describe("frames", () => {
  it("shows the last 10 s of frames, a few times a second", () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    frames(0, 240, 16);
    expect(hud.frames).toBeUndefined();
    frame(256);
    expect(hud.frames).toMatchObject({ latest: 16, worst: 16, overLimit: 0 });
    frame(296);
    frames(312, 12_000, 16);
    expect(hud.frames?.worst).toBe(16);
  });

  it("doesn't count the time the window was hidden as a frame", () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    frames(0, 256, 16);
    document.dispatchEvent(new Event("visibilitychange"));
    frames(5000, 5240, 16);
    frame(5300);
    expect(hud.frames?.worst).toBe(60);
  });
});

describe("keys", () => {
  it("times a key to the frame after it", () => {
    const { hud, frame, setNow } = harness();
    hud.toggle();
    frame(0);
    hud.keyPressed(3);
    setNow(12);
    hud.keyPressed(5);
    frame(16.5);
    expect(hud.key).toBe(13.5);
  });

  it("stops the clock once the key's frame has painted", () => {
    const { hud, clock, frame, setNow } = harness();
    let painted = () => {};
    clock.afterPaint.mockImplementation((callback: () => void) => {
      painted = callback;
    });
    hud.toggle();
    frame(0);
    hud.keyPressed(4);
    frame(16);
    expect(hud.key).toBeUndefined();
    setNow(30);
    painted();
    expect(hud.key).toBe(26);
  });

  it("takes no key while hidden", () => {
    const { hud, frame } = harness();
    hud.keyPressed(3);
    hud.toggle();
    frame(16);
    frame(32);
    expect(hud.key).toBeUndefined();
  });

  it("listens for keys only while it shows", () => {
    const { hud, frame } = harness();
    const press = () => window.dispatchEvent(new KeyboardEvent("keydown", { key: "j" }));
    press();
    hud.toggle();
    frame(0);
    expect(hud.key).toBeUndefined();
    press();
    frame(16);
    expect(hud.key).toBeTypeOf("number");
    hud.toggle();
    press();
    hud.toggle();
    frame(32);
    frame(48);
    expect(hud.key).toBeUndefined();
  });
});

describe("view queries", () => {
  it("times what the window waited for, even while hidden", async () => {
    const { hud, setNow } = harness();
    setNow(100);
    const answer = hud.timed(
      () =>
        new Promise<string>((resolve) => {
          setNow(131);
          resolve("rows");
        }),
    );
    expect(await answer).toBe("rows");
    expect(hud.viewQuery).toBe(31);
  });

  it("times a query that failed", async () => {
    const { hud, setNow } = harness();
    setNow(10);
    const failing = hud.timed(
      () =>
        new Promise((_, reject) => {
          setNow(20);
          reject(new Error("no database"));
        }),
    );
    await expect(failing).rejects.toThrow("no database");
    expect(hud.viewQuery).toBe(10);
  });
});

describe("start-up", () => {
  it("asks the core how long it has run once the Workbench is usable, and only the first time", async () => {
    const { hud, core, setNow } = harness();
    setNow(100);
    core.timeRunning.mockImplementationOnce(() => {
      setNow(110);
      return Promise.resolve(640);
    });
    await hud.markUsable();
    expect(hud.startup).toBe(635);
    await hud.markUsable();
    expect(hud.startup).toBe(635);
    expect(core.timeRunning).toHaveBeenCalledOnce();
  });

  it("has no start-up time when the core can't say", async () => {
    const { hud, core } = harness();
    core.timeRunning.mockResolvedValueOnce(null);
    await hud.markUsable();
    expect(hud.startup).toBeUndefined();
  });

  it("has no start-up time for a run that opened on something else", async () => {
    const { hud, core } = harness();
    hud.skipStartup();
    await hud.markUsable();
    expect(hud.startup).toBeUndefined();
    expect(core.timeRunning).not.toHaveBeenCalled();
  });

  it("says why it has no start-up time", async () => {
    const { hud, core } = harness();
    core.timeRunning.mockRejectedValueOnce(new Error("no core"));
    await hud.markUsable();
    expect(hud.startup).toBeUndefined();
    expect(hud.error).toBe("Error: no core");
  });
});

describe("background refreshes", () => {
  it("counts the frames dropped during a refresh", () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    hud.refreshing(false);
    frames(0, 320, 16);
    hud.refreshing(true);
    frame(368);
    frame(384);
    hud.refreshing(false);
    frame(400);
    frame(448);
    expect(hud.lastRefresh).toBe(2);
  });

  it("doesn't count a refresh it didn't see start", () => {
    const { hud, frames, frame } = harness();
    hud.refreshing(false);
    hud.refreshing(true);
    hud.toggle();
    hud.refreshing(true);
    frames(0, 320, 16);
    frame(368);
    hud.refreshing(false);
    expect(hud.lastRefresh).toBeUndefined();
  });

  it("doesn't count a refresh that ends after it hid", () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    hud.refreshing(false);
    hud.refreshing(true);
    frames(0, 320, 16);
    frame(368);
    hud.toggle();
    hud.refreshing(false);
    expect(hud.lastRefresh).toBeUndefined();
  });
});

describe("recording", () => {
  it("writes what it measured between Start and Stop to the log", async () => {
    const { hud, core, frames, frame, setNow } = harness();
    await hud.markUsable();
    hud.toggle();
    frames(0, 320, 16);
    hud.start();
    expect(hud.recording).toBe(true);
    frame(336);
    frame(376);
    hud.keyPressed(370);
    frame(392);
    await hud.timed(() => Promise.resolve());
    await hud.timed(() => {
      setNow(422);
      return Promise.resolve();
    });
    hud.refreshing(false);
    hud.refreshing(true);
    frame(440);
    hud.refreshing(false);
    setNow(1320);
    await hud.stop();

    expect(hud.recording).toBe(false);
    expect(core.recordMeasurement).toHaveBeenCalledWith({
      seconds: 1,
      frames: { count: 4, p95_ms: 48, worst_ms: 48, limit_ms: 25, over_limit: 2 },
      startup_ms: 640,
      keys: { count: 1, worst_ms: 22 },
      view_queries: { count: 2, worst_ms: 30 },
      refreshes: { count: 1, dropped_frames: 2 },
      issues: null,
      long_frames: [
        { at_s: 0.056, ms: 40, cause: null },
        { at_s: 0.12, ms: 48, cause: { kind: "view_query" } },
      ],
    });
    expect(hud.written).toBe("the line");
  });

  it("records only while it shows", async () => {
    const { hud, core } = harness();
    hud.start();
    expect(hud.recording).toBe(false);
    await hud.stop();
    expect(core.recordMeasurement).not.toHaveBeenCalled();
  });

  it("sends nothing for what it didn't see", async () => {
    const { hud, core } = harness();
    hud.toggle();
    hud.start();
    await hud.stop();
    expect(core.recordMeasurement).toHaveBeenCalledWith({
      seconds: 0,
      frames: null,
      startup_ms: null,
      keys: null,
      view_queries: null,
      refreshes: null,
      issues: null,
      long_frames: [],
    });
  });

  it("writes a recording to the log when hidden", async () => {
    const { hud, core } = harness();
    hud.toggle();
    hud.start();
    hud.toggle();
    expect(hud.recording).toBe(false);
    await vi.waitFor(() => {
      expect(hud.written).toBe("the line");
    });
    await hud.stop();
    expect(core.recordMeasurement).toHaveBeenCalledOnce();
  });

  it("shows nothing old when shown again", async () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    hud.refreshing(false);
    hud.refreshing(true);
    frames(0, 320, 16);
    hud.keyPressed(310);
    frame(336);
    hud.refreshing(false);
    expect([hud.frames, hud.key, hud.lastRefresh]).not.toContain(undefined);
    hud.toggle();
    await hud.timed(() => Promise.resolve());
    hud.toggle();
    expect([hud.frames, hud.key, hud.lastRefresh]).toEqual([undefined, undefined, undefined]);
    expect(hud.viewQuery).toBe(0);
  });

  it("says why the line wasn't written", async () => {
    const { hud, core } = harness();
    core.recordMeasurement.mockRejectedValueOnce(new Error("no log"));
    hud.toggle();
    hud.start();
    await hud.stop();
    expect(hud.written).toBeUndefined();
    expect(hud.error).toBe("Error: no log");
  });
});

describe("the browser's clock", () => {
  it("draws frames, can stop one, and calls back after a frame has painted", async () => {
    const drawn = await new Promise<number>((resolve) => browserClock.requestFrame(resolve));
    expect(drawn).toBeTypeOf("number");
    const never = vi.fn();
    browserClock.cancelFrame(browserClock.requestFrame(never));
    await new Promise<void>((resolve) => {
      browserClock.afterPaint(resolve);
    });
    await new Promise((resolve) => browserClock.requestFrame(resolve));
    expect(never).not.toHaveBeenCalled();
    expect(browserClock.now()).toBeGreaterThan(0);
  });
});

/** Clicks the window at `at` on the HUD's clock, which a real click gets from the browser. */
function clickAt(at: number) {
  const click = new MouseEvent("click");
  Object.defineProperty(click, "timeStamp", { value: at });
  window.dispatchEvent(click);
}

describe("long frames", () => {
  it("shows the last frame over 25 ms with what came before it, after 10 s have passed", async () => {
    const { hud, frames, frame, setNow } = harness();
    hud.toggle();
    frames(0, 320, 16);
    setNow(330);
    await hud.timed(() => Promise.resolve());
    frame(360);
    expect(hud.longFrame).toEqual({ end: 360, duration: 40, cause: { kind: "view_query" } });
    frames(376, 12_000, 16);
    expect(hud.frames?.worst).toBe(16);
    expect(hud.longFrame?.end).toBe(360);
    expect(hud.now).toBeGreaterThan(11_700);
  });

  it("takes a key or a click as a cause, and a frame with neither has none", () => {
    const { hud, frames, frame } = harness();
    hud.toggle();
    frames(0, 320, 16);
    hud.keyPressed(330);
    frame(360);
    expect(hud.longFrame?.cause).toEqual({ kind: "key" });
    frames(376, 2000, 16);
    clickAt(2010);
    frame(2040);
    expect(hud.longFrame?.cause).toEqual({ kind: "click" });
    frames(2056, 4000, 16);
    frame(4040);
    expect(hud.longFrame?.cause).toBeUndefined();
  });

  it("hears clicks only while it shows", () => {
    const { hud, frame } = harness();
    clickAt(1010);
    hud.toggle();
    hud.toggle();
    clickAt(1020);
    hud.toggle();
    frame(1000);
    frame(1040);
    expect(hud.longFrame).toEqual({ end: 1040, duration: 40, cause: undefined });
  });
});

describe("opening an Issue", () => {
  it("times an Issue the database holds from the click to its painted frame", () => {
    const { hud, frame, setNow } = harness();
    hud.toggle();
    frame(0);
    setNow(5);
    hud.issueOpened(1234);
    expect(hud.issue).toEqual({ stage: "database", number: 1234 });
    hud.issueShown(1234, true);
    frame(16);
    expect(hud.issue).toEqual({ stage: "held", number: 1234, fromDatabase: 11 });
  });

  it("times the text that had to come from GitHub, and names it as a cause", async () => {
    const { hud, frame, frames, setNow } = harness();
    hud.toggle();
    frames(0, 320, 16);
    setNow(330);
    hud.issueOpened(1234);
    hud.issueShown(1234, false);
    frame(336);
    expect(hud.issue).toEqual({ stage: "github", number: 1234, fromDatabase: 6 });
    hud.issueShown(1234, false);
    setNow(900);
    hud.issueShown(1234, true);
    frame(940);
    await Promise.resolve();
    const read = { stage: "read", number: 1234, fromDatabase: 6, fromGitHub: 610 };
    expect(hud.issue).toEqual(read);
    expect(hud.longFrame?.cause).toEqual({ kind: "issue_text", number: 1234 });
    hud.issueShown(1234, true);
    frame(956);
    await Promise.resolve();
    expect(hud.issue).toEqual(read);
  });

  it("leaves out another Issue's answer", () => {
    const { hud, frame } = harness();
    hud.toggle();
    frame(0);
    hud.issueOpened(2);
    hud.issueShown(1, true);
    frame(16);
    expect(hud.issue).toEqual({ stage: "database", number: 2 });
  });

  it("says in a recording the slowest Issue from the database and from GitHub", async () => {
    const { hud, core, frame, setNow } = harness();
    hud.toggle();
    frame(0);
    hud.start();
    hud.issueOpened(1);
    hud.issueShown(1, true);
    frame(16);
    hud.issueOpened(2);
    hud.issueShown(2, false);
    frame(48);
    setNow(500);
    hud.issueShown(2, true);
    frame(512);
    await Promise.resolve();
    await hud.stop();
    expect(core.recordMeasurement).toHaveBeenCalledWith(
      expect.objectContaining({
        issues: { from_database: { count: 2, worst_ms: 32 }, from_github: { count: 1, worst_ms: 496 } },
      }),
    );
  });

  it("still counts an Issue whose text never arrived", async () => {
    const { hud, core, frame } = harness();
    hud.toggle();
    frame(0);
    hud.start();
    hud.issueOpened(1);
    hud.issueShown(1, false);
    frame(250);
    await hud.stop();
    expect(core.recordMeasurement).toHaveBeenCalledWith(
      expect.objectContaining({ issues: { from_database: { count: 1, worst_ms: 250 }, from_github: null } }),
    );
  });

  it("drops what waits for a paint when the window hides, so the hidden time doesn't count", () => {
    const { hud, frame, frames } = harness();
    const hidden = vi.spyOn(document, "hidden", "get");
    hud.toggle();
    frame(0);
    hud.issueOpened(1);
    hud.issueShown(1, false);
    hud.keyPressed(1);
    hidden.mockReturnValue(true);
    document.dispatchEvent(new Event("visibilitychange"));
    hidden.mockReturnValue(false);
    document.dispatchEvent(new Event("visibilitychange"));
    frames(60_000, 60_016, 16);
    expect(hud.issue).toBeUndefined();
    expect(hud.key).toBeUndefined();
    hud.issueShown(1, true);
    frame(60_032);
    expect(hud.issue).toBeUndefined();
    hidden.mockRestore();
  });

  it("keeps the last Issue's times when the window hides after it opened", () => {
    const { hud, frame } = harness();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    hud.toggle();
    frame(0);
    hud.issueOpened(1);
    hud.issueShown(1, true);
    frame(16);
    document.dispatchEvent(new Event("visibilitychange"));
    expect(hud.issue).toEqual({ stage: "held", number: 1, fromDatabase: 16 });
    hidden.mockRestore();
  });
});

describe("the misses it logs, shown or not", () => {
  it("logs a view query over its budget", async () => {
    const { hud, core, setNow } = harness();
    const query = (ms: number) =>
      hud.timed(() => {
        setNow(ms);
        return Promise.resolve();
      });
    setNow(0);
    await query(50);
    expect(core.recordMiss).not.toHaveBeenCalled();
    setNow(0);
    await query(73);
    expect(core.recordMiss).toHaveBeenCalledExactlyOnceWith({ kind: "view_query", ms: 73 });
  });

  it("logs the start-up, fast or slow", async () => {
    const { hud, core } = harness();
    await hud.markUsable();
    expect(core.recordMiss).toHaveBeenCalledExactlyOnceWith({ kind: "startup", ms: 640 });
  });

  it("logs no start-up the core can't say", async () => {
    const { hud, core } = harness();
    core.timeRunning.mockResolvedValueOnce(null);
    await hud.markUsable();
    expect(core.recordMiss).not.toHaveBeenCalled();
  });

  it("times an Issue opened while hidden with one frame for each part, and logs the slow parts", async () => {
    const { hud, core, clock, frame, waiting, setNow } = harness();
    setNow(0);
    hud.issueOpened(7);
    hud.issueShown(7, false);
    expect(clock.requestFrame).toHaveBeenCalledOnce();
    frame(250);
    expect(core.recordMiss).toHaveBeenCalledExactlyOnceWith({
      kind: "issue_from_database",
      number: 7,
      ms: 250,
    });
    expect(waiting.size).toBe(0);
    setNow(2600);
    hud.issueShown(7, true);
    frame(2610);
    await Promise.resolve();
    expect(core.recordMiss).toHaveBeenLastCalledWith({ kind: "issue_from_github", number: 7, ms: 2610 });
    expect(waiting.size).toBe(0);
  });

  it("drops an Issue waiting for GitHub when the window hides", () => {
    const { hud, core, frame, waiting } = harness();
    const hidden = vi.spyOn(document, "hidden", "get");
    hud.issueOpened(7);
    hud.issueShown(7, false);
    frame(100);
    hidden.mockReturnValue(true);
    document.dispatchEvent(new Event("visibilitychange"));
    hidden.mockReturnValue(false);
    document.dispatchEvent(new Event("visibilitychange"));
    hud.issueShown(7, true);
    expect(waiting.size).toBe(0);
    expect(core.recordMiss).not.toHaveBeenCalled();
    hidden.mockRestore();
  });

  it("keeps timing an Issue when the HUD hides", () => {
    const { hud, core, frame, setNow } = harness();
    hud.toggle();
    frame(0);
    hud.issueOpened(7);
    hud.issueShown(7, true);
    hud.toggle();
    setNow(300);
    frame(300);
    expect(core.recordMiss).toHaveBeenCalledExactlyOnceWith({
      kind: "issue_from_database",
      number: 7,
      ms: 300,
    });
  });

  it("logs no view query the window hid during", async () => {
    const { hud, core, setNow } = harness();
    const hidden = vi.spyOn(document, "hidden", "get");
    setNow(0);
    await hud.timed(() => {
      setNow(400);
      hidden.mockReturnValue(true);
      document.dispatchEvent(new Event("visibilitychange"));
      hidden.mockReturnValue(false);
      return Promise.resolve();
    });
    setNow(0);
    hidden.mockReturnValue(true);
    await hud.timed(() => {
      setNow(400);
      return Promise.resolve();
    });
    expect(core.recordMiss).not.toHaveBeenCalled();
    hidden.mockRestore();
  });

  it("hands an Issue's paint to the frame loop when it shows", () => {
    const { hud, clock, frame, setNow } = harness();
    setNow(0);
    hud.issueOpened(7);
    hud.issueShown(7, true);
    hud.toggle();
    expect(clock.cancelFrame).toHaveBeenCalledExactlyOnceWith(1);
    frame(16);
    expect(hud.issue).toEqual({ stage: "held", number: 7, fromDatabase: 16 });
  });

  it("logs nothing for an Issue opened in time", async () => {
    const { hud, core, frame, setNow } = harness();
    setNow(0);
    hud.issueOpened(7);
    hud.issueShown(7, false);
    frame(200);
    setNow(1990);
    hud.issueShown(7, true);
    frame(2000);
    await Promise.resolve();
    expect(core.recordMiss).not.toHaveBeenCalled();
  });

  it("drops the frame it asked for while hidden when the window hides", () => {
    const { hud, core, waiting } = harness();
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    hud.issueOpened(7);
    hud.issueShown(7, true);
    document.dispatchEvent(new Event("visibilitychange"));
    expect(waiting.size).toBe(0);
    hidden.mockRestore();
    document.dispatchEvent(new Event("visibilitychange"));
    hud.issueShown(7, true);
    expect(waiting.size).toBe(0);
    expect(core.recordMiss).not.toHaveBeenCalled();
  });
});
