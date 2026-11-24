// Clocks that move words such as "3m ago" and "4 s" on by themselves.
import { SvelteDate } from "svelte/reactivity";
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

/** The time now, set again each minute while the component that calls this as it starts is on screen, so
 * words such as "3m ago" move on between syncs. A window left open, hidden or minimized, can have its
 * timers throttled or stopped by the WebView, so the clock also follows the system's each time the window
 * comes back into view or takes the focus (#1066). */
export function minuteClock(): Date {
  // SvelteDate extends the Date there was at import, so a test's fake clock reaches it only through `now()`.
  const now = new SvelteDate(Date.now());
  $effect(() => {
    const tick = () => {
      now.setTime(Date.now());
    };
    const ticking = setInterval(tick, MINUTE_MS);
    // The window's own returns: a hidden window's visibility becomes "visible" again, and a minimized
    // one takes the focus.
    const back = () => {
      if (!document.hidden) tick();
    };
    document.addEventListener("visibilitychange", back);
    window.addEventListener("focus", back);
    return () => {
      clearInterval(ticking);
      document.removeEventListener("visibilitychange", back);
      window.removeEventListener("focus", back);
    };
  });
  return now;
}
