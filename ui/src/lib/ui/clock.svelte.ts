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

/** The time now, set again each minute while the component that calls this as it starts is on screen, so
 * words such as "3m ago" move on between syncs, and again when the window comes back into view or takes the
 * focus.
 * The time is a plain number, not a SvelteDate: a derived that read a SvelteDate's time once kept that time
 * for good, so the window's words stopped at launch (#1066). */
export function minuteClock(): { readonly now: Date } {
  let at = $state(Date.now());
  $effect(() => {
    const tick = () => {
      at = Date.now();
    };
    const ticking = setInterval(tick, MINUTE_MS);
    document.addEventListener("visibilitychange", tick);
    window.addEventListener("focus", tick);
    return () => {
      clearInterval(ticking);
      document.removeEventListener("visibilitychange", tick);
      window.removeEventListener("focus", tick);
    };
  });
  return {
    get now() {
      // Made anew on each read, so no reader can change it, and the `$state` above is what's reactive.
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      return new Date(at);
    },
  };
}
