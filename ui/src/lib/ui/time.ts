// The time since something happened, in the one unit the window words it in, and the date and the time it
// happened at, which a tooltip shows beside it (#1066).

/** A second, in milliseconds. */
export const SECOND_MS = 1000;
/** A minute, in milliseconds. */
export const MINUTE_MS = 60_000;
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

const AGO_UNITS = { minute: "min", hour: "h", day: "d" } as const;

/** The time from `since` to `now` in words: "just now", "3 min ago", "2 h ago" or "4 d ago". */
export function agoWords(since: string, now: Date): string {
  const time = elapsed(since, now);
  return time.unit === "now" ? "just now" : `${String(time.count)} ${AGO_UNITS[time.unit]} ago`;
}

/** The date and the time of `at`, as the system's format shows them: "Nov 23, 2026, 3:42 AM". A tooltip
 * says it, so a bare "2 h ago" tells the user when the moment was. */
export function dateTime(at: string | number): string {
  return new Date(at).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}
