// A clock that moves words such as "3m ago" on by themselves.
import { SvelteDate } from "svelte/reactivity";

const MINUTE_MS = 60_000;

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
