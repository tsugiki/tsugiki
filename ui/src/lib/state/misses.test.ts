import { describe, expect, it, vi } from "vitest";
import type { Miss } from "$lib/data/bindings";
import { CAP_PER_MINUTE, MissLog } from "./misses";

function harness() {
  let now = 0;
  const core = { recordMiss: vi.fn<(miss: Miss) => Promise<string>>(() => Promise.resolve("the line")) };
  const misses = new MissLog({ now: () => now }, core);
  const logged = () => core.recordMiss.mock.calls.map(([miss]) => miss);
  return {
    misses,
    core,
    logged,
    setNow: (time: number) => {
      now = time;
    },
  };
}

describe("the cap on repeats", () => {
  it("logs each miss up to the cap in a minute, then says it stops", () => {
    const { misses, logged } = harness();
    for (let i = 0; i < CAP_PER_MINUTE + 3; i++) misses.missed({ kind: "view_query", ms: 60 + i });
    expect(logged()).toHaveLength(CAP_PER_MINUTE + 1);
    expect(logged().at(-2)).toEqual({ kind: "view_query", ms: 60 + CAP_PER_MINUTE - 1 });
    expect(logged().at(-1)).toEqual({ kind: "capped", of: "view_query", limit: CAP_PER_MINUTE });
  });

  it("logs again once the minute has passed", () => {
    const { misses, logged, setNow } = harness();
    for (let i = 0; i < CAP_PER_MINUTE + 1; i++) misses.missed({ kind: "view_query", ms: 60 });
    setNow(59_999);
    misses.missed({ kind: "view_query", ms: 61 });
    setNow(60_000);
    misses.missed({ kind: "view_query", ms: 62 });
    expect(logged().at(-1)).toEqual({ kind: "view_query", ms: 62 });
    expect(logged()).toHaveLength(CAP_PER_MINUTE + 2);
  });

  it("caps each kind on its own", () => {
    const { misses, logged } = harness();
    for (let i = 0; i < CAP_PER_MINUTE + 1; i++) misses.missed({ kind: "view_query", ms: 60 });
    misses.missed({ kind: "long_frame", ms: 80, scripts: [] });
    expect(logged().at(-1)).toEqual({ kind: "long_frame", ms: 80, scripts: [] });
  });

  it("logs the start-up whatever it took", () => {
    const { misses, logged } = harness();
    misses.startup(400);
    expect(logged()).toEqual([{ kind: "startup", ms: 400 }]);
  });

  it("carries on when the core can't write the line", async () => {
    const { misses, core, logged } = harness();
    core.recordMiss.mockRejectedValueOnce(new Error("no log"));
    misses.missed({ kind: "view_query", ms: 60 });
    await Promise.resolve();
    misses.missed({ kind: "view_query", ms: 61 });
    expect(logged()).toHaveLength(2);
  });
});

/** A stand-in for the browser's `PerformanceObserver`, which hands the test each observer it made. */
function fakeObservers(supported: readonly string[]) {
  const made: { type: string; threshold: number | undefined; report: (entries: unknown[]) => void }[] = [];
  class Observer {
    static supportedEntryTypes = supported;
    #callback: (list: { getEntries(): unknown[] }) => void;
    constructor(callback: (list: { getEntries(): unknown[] }) => void) {
      this.#callback = callback;
    }
    observe(options: { type: string; durationThreshold?: number }) {
      made.push({
        type: options.type,
        threshold: options.durationThreshold,
        report: (entries) => {
          this.#callback({ getEntries: () => entries });
        },
      });
    }
  }
  const report = (type: string, entries: unknown[]) => {
    for (const observer of made.filter((o) => o.type === type)) observer.report(entries);
  };
  return { Observer, made, report };
}

describe("what the browser reports", () => {
  it("logs a key or click slower than a frame, once for each", () => {
    const { misses, logged } = harness();
    const { Observer, report } = fakeObservers(["event", "long-animation-frame"]);
    misses.watch(Observer);
    report("event", [
      { name: "keydown", duration: 48, interactionId: 1 },
      { name: "keyup", duration: 40, interactionId: 1 },
      { name: "pointerdown", duration: 32, interactionId: 2 },
      { name: "click", duration: 32, interactionId: 2 },
      { name: "keydown", duration: 16, interactionId: 3 },
      { name: "mousemove", duration: 104, interactionId: 0 },
      { name: "beforeinput", duration: 56, interactionId: 4 },
    ]);
    expect(logged()).toEqual([
      { kind: "input", input: "key", ms: 48 },
      { kind: "input", input: "click", ms: 32 },
    ]);
  });

  it("asks only for the events that can miss", () => {
    const { misses } = harness();
    const { Observer, made } = fakeObservers(["event", "long-animation-frame"]);
    misses.watch(Observer);
    expect(made.map(({ type, threshold }) => [type, threshold])).toEqual([
      ["event", 32],
      ["long-animation-frame", undefined],
    ]);
  });

  it("logs a long frame with its three longest scripts, named without the window's origin", () => {
    const { misses, logged } = harness();
    const { Observer, report } = fakeObservers(["event", "long-animation-frame"]);
    misses.watch(Observer);
    const script = (invoker: string, duration: number, sourceURL = "http://tauri.localhost/_app/a.js") => ({
      invoker,
      sourceFunctionName: "run",
      sourceURL,
      duration,
    });
    report("long-animation-frame", [
      {
        duration: 82,
        scripts: [
          script("FrameRequestCallback", 5),
          script("HTMLDivElement.onkeydown", 60),
          script("Response.json.then", 9, ""),
          script("TimerHandler:setTimeout", 7),
        ],
      },
    ]);
    expect(logged()).toEqual([
      {
        kind: "long_frame",
        ms: 82,
        scripts: [
          { invoker: "HTMLDivElement.onkeydown", function: "run", source: "/_app/a.js", ms: 60 },
          { invoker: "Response.json.then", function: "run", source: null, ms: 9 },
          { invoker: "TimerHandler:setTimeout", function: "run", source: "/_app/a.js", ms: 7 },
        ],
      },
    ]);
  });

  it("logs each interaction once when another comes between its entries", () => {
    const { misses, logged } = harness();
    const { Observer, report } = fakeObservers(["event"]);
    misses.watch(Observer);
    report("event", [
      { name: "keydown", duration: 48, interactionId: 1 },
      { name: "pointerdown", duration: 40, interactionId: 2 },
      { name: "keyup", duration: 40, interactionId: 1 },
    ]);
    report("event", [{ name: "click", duration: 40, interactionId: 2 }]);
    expect(logged()).toEqual([
      { kind: "input", input: "key", ms: 48 },
      { kind: "input", input: "click", ms: 40 },
    ]);
  });

  it("names no function the browser doesn't know", () => {
    const { misses, logged } = harness();
    const { Observer, report } = fakeObservers(["long-animation-frame"]);
    misses.watch(Observer);
    const scripts = [
      { invoker: "FrameRequestCallback", sourceFunctionName: "", sourceURL: "", duration: 51 },
      { invoker: "Window.eval", sourceFunctionName: "", sourceURL: "inline", duration: 4 },
    ];
    report("long-animation-frame", [{ duration: 60, scripts }]);
    expect(logged()).toEqual([
      {
        kind: "long_frame",
        ms: 60,
        scripts: [
          { invoker: "FrameRequestCallback", function: null, source: null, ms: 51 },
          { invoker: "Window.eval", function: null, source: "inline", ms: 4 },
        ],
      },
    ]);
  });

  it("watches once, so a miss isn't logged twice", () => {
    const { misses } = harness();
    const { Observer, made } = fakeObservers(["event", "long-animation-frame"]);
    misses.watch(Observer);
    misses.watch(Observer);
    expect(made).toHaveLength(2);
  });

  it("watches only what the browser has, as WKWebView has neither", () => {
    const { misses } = harness();
    const { Observer, made } = fakeObservers([]);
    misses.watch(Observer);
    expect(made).toEqual([]);
    expect(() => {
      misses.watch(undefined);
    }).not.toThrow();
  });
});
