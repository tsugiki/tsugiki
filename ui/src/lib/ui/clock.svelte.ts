// Clocks that move words such as "3m ago" and "4 s" on by themselves.
import { SvelteDate } from "svelte/reactivity";
import { MINUTE_MS, SECOND_MS } from "./time";

/** The time now in milliseconds, set again on each whole second after the moment `since` gives, while it
 * gives one and the component that calls this as it starts is on screen, so a count of seconds since then
 * moves on by itself. */
export function secondClock(since: () => string | null): { readonly now: number } {
  let now = $state(Date.now());
  $effect(() => {
    const from = since();
    if (from === null) return;
    const start = Date.parse(from);
    let ticking: ReturnType<typeof setTimeout> | undefined;
    const tick = () => {
      const at = Date.now();
      now = at;
      // To the next whole second after `start`, which a late timer doesn't push back.
      ticking = setTimeout(tick, SECOND_MS - ((at - start) % SECOND_MS));
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
 * words such as "3m ago" move on between syncs. */
export function minuteClock(): Date {
  // SvelteDate extends the Date there was at import, so a test's fake clock reaches it only through `now()`.
  const now = new SvelteDate(Date.now());
  $effect(() => {
    const ticking = setInterval(() => {
      now.setTime(Date.now());
    }, MINUTE_MS);
    return () => {
      clearInterval(ticking);
    };
  });
  return now;
}
