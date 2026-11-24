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

/** The window's clock: the time now, as the system says it. */
export type Clock = { readonly now: Date };

/** The time now, set again each minute while the component that calls this as it starts is on screen, so
 * words such as "3m ago" move on between syncs.
 *
 * The clock holds a plain value, which every reader sees. A `SvelteDate` keeps its time in a derived of
 * its own, and that derived stops moving once the effect that first read it goes away: the window then
 * keeps the time it opened at, and every moment since reads as the future, which `elapsed()` calls "just
 * now" (#1066).
 *
 * A hidden or minimized window's timers may be throttled or stopped, so the clock reads the system's time
 * again as soon as the window comes back to the front, as `window-shown.ts` watches for the core. */
export function minuteClock(): Clock {
  let at = $state(Date.now());
  const read = () => {
    at = Date.now();
  };
  $effect(() => {
    const ticking = setInterval(read, MINUTE_MS);
    const shown = () => read();
    document.addEventListener("visibilitychange", shown);
    return () => {
      clearInterval(ticking);
      document.removeEventListener("visibilitychange", shown);
    };
  });
  return {
    get now() {
      // The Date is made afresh on each read and never held or changed, and a `SvelteDate` keeps its time in
      // a derived of its own that stops moving (#1066).
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      return new Date(at);
    },
  };
}
