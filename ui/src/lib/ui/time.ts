// The time since something happened, in the one unit the window words it in.

const MINUTE_MS = 60_000;
const MINUTES_IN_HOUR = 60;
const MINUTES_IN_DAY = 24 * MINUTES_IN_HOUR;

/** The time from one moment to another, in whole minutes under an hour, whole hours under a day, and whole
 * days from then on. Under a minute is now, as is a time in the future, from a clock behind GitHub's. */
export type Elapsed = { unit: "now" } | { unit: "minute" | "hour" | "day"; count: number };

/** The time from `since` to `now`, rounded down to its unit. */
export function elapsed(since: string, now: Date): Elapsed {
  const minutes = Math.floor((now.getTime() - new Date(since).getTime()) / MINUTE_MS);
  if (minutes < 1) return { unit: "now" };
  if (minutes < MINUTES_IN_HOUR) return { unit: "minute", count: minutes };
  if (minutes < MINUTES_IN_DAY) return { unit: "hour", count: Math.floor(minutes / MINUTES_IN_HOUR) };
  return { unit: "day", count: Math.floor(minutes / MINUTES_IN_DAY) };
}
