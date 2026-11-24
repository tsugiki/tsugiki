// Clocks that move words such as "3m ago" and "4 s" on by themselves.
import { MINUTE_MS, SECOND_MS } from "./time";

/** The time now in milliseconds, set as the moment `since` gives changes, then again `after` milliseconds
 * past that moment and on each whole second after it, while it gives one and the component that calls this
 * as it starts is on screen, so a count of seconds since then moves on by itself once it shows. */
export function secondClock(since: () => string | null, after: number): { readonly now: number } {
  let now = $state(Date.now());
  $effect(() => {
    const from = since();
    if (from === null) return;
    const start = Date.parse(from);
    let ticking: ReturnType<typeof setTimeout> | undefined;
    const tick = () => {
      const at = Date.now();
      now = at;
      const waited = at - start;
      // To `after`, then to the next whole second after `start`, which a late timer doesn't push back.
      const next = waited < after ? after - waited : SECOND_MS - (waited % SECOND_MS);
      ticking = setTimeout(tick, next);
    };
    tick();
    return () => {
      clearTimeout(ticking);
    };
  });
  return {
    get now() {
      return now;
    },
  };
}

/** The window's clock. Its time is always the system's: each read takes `Date.now()`, so however long the
 * window has been open, hidden or minimized, an age reads against the time the reader asked. `wake` only
 * wakes the readers, as a sync or any other change on screen does too; it never hands out a time. */
export class SystemClock extends Date {
  /** How often the clock has woken its readers. Each read takes it, so a wake re-renders every age on
   * screen. */
  #woken = $state(0);

  /** Wakes the readers, which then read the system's time afresh. */
  wake(): void {
    this.#woken += 1;
  }

  override getTime(): number {
    void this.#woken;
    return Date.now();
  }
}

/** The time now, woken each minute and each time the window returns, so words such as "3m ago" move on
 * between syncs and are true the moment the window shows again. A hidden or minimized window's timers the
 * WebView may throttle or stop: a wake from the window's return catches up at once, and every read of the
 * clock between wakes is the system's own. */
export function minuteClock(): Date {
  const now = new SystemClock();
  $effect(() => {
    const wake = () => now.wake();
    const ticking = setInterval(wake, MINUTE_MS);
    document.addEventListener("visibilitychange", wake);
    window.addEventListener("focus", wake);
    return () => {
      clearInterval(ticking);
      document.removeEventListener("visibilitychange", wake);
      window.removeEventListener("focus", wake);
    };
  });
  return now;
}
