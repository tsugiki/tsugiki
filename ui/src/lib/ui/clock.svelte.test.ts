import { flushSync } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { minuteClock } from "./clock.svelte";
import { agoWords, MINUTE_MS } from "./time";

/** The moment the window opened, and the moment of the comment it words. */
const OPENED = new Date("2026-11-22T07:05:00Z");
const SINCE = "2026-11-22T06:00:00Z";
const DAY_MS = 24 * 60 * MINUTE_MS;

/** A window's clock as a component reads it: one derived of the clock's time, which redraws with it, and the
 * clock stopping as the component goes, as `minuteClock` is called as a component starts. */
function windowClock() {
  let clock!: { readonly now: Date };
  let shown!: () => string;
  const stop = $effect.root(() => {
    clock = minuteClock();
    const words = $derived(agoWords(SINCE, clock.now));
    shown = () => words;
  });
  flushSync();
  return { now: () => clock.now.toISOString(), shown: () => shown(), stop };
}

afterEach(() => {
  vi.useRealTimers();
});

it("keeps a time true while the window stays open for a day", async () => {
  vi.useFakeTimers({ now: OPENED });
  const clock = windowClock();
  expect(clock.shown()).toBe("1 h ago");

  await vi.advanceTimersByTimeAsync(DAY_MS);
  flushSync();

  expect(clock.now()).toBe("2026-11-23T07:05:00.000Z");
  expect(clock.shown()).toBe("1 d ago");
  clock.stop();
});

it("reads the time again when the window comes back into view", () => {
  vi.useFakeTimers({ now: OPENED });
  const clock = windowClock();

  // A hidden window may let no timer run, so the time moves on with nothing ticking.
  vi.setSystemTime(new Date(OPENED.getTime() + DAY_MS));
  flushSync();
  expect(clock.shown()).toBe("1 h ago");

  document.dispatchEvent(new Event("visibilitychange"));
  flushSync();

  expect(clock.shown()).toBe("1 d ago");
  clock.stop();
});

it("reads the time again when the window takes the focus", () => {
  vi.useFakeTimers({ now: OPENED });
  const clock = windowClock();

  // A minimized window may let no timer run, so the time moves on with nothing ticking.
  vi.setSystemTime(new Date(OPENED.getTime() + DAY_MS));
  flushSync();
  expect(clock.shown()).toBe("1 h ago");

  window.dispatchEvent(new Event("focus"));
  flushSync();

  expect(clock.shown()).toBe("1 d ago");
  clock.stop();
});

it("stops its timer when the component goes", () => {
  vi.useFakeTimers();
  const clock = windowClock();
  expect(vi.getTimerCount()).toBe(1);
  clock.stop();
  expect(vi.getTimerCount()).toBe(0);
});

it("stops hearing the window when the component goes", () => {
  vi.useFakeTimers({ now: OPENED });
  const clock = windowClock();
  expect(clock.now()).toBe("2026-11-22T07:05:00.000Z");

  clock.stop();
  // Neither a window coming back into view nor one taking the focus moves a clock whose component has gone.
  vi.setSystemTime(new Date(OPENED.getTime() + DAY_MS));
  document.dispatchEvent(new Event("visibilitychange"));
  window.dispatchEvent(new Event("focus"));
  flushSync();

  expect(clock.now()).toBe("2026-11-22T07:05:00.000Z");
});
