// The time since something happened, in the one unit the window words it in.

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

/** The moment `milliseconds` past the epoch names. The window's clock gives one on each read, so no
 * reader holds a Date that another could change. */
export const at = (milliseconds: number): Date => new Date(milliseconds);

/** The date and the time of `when`, in the system's format: "23 Nov 2026, 03:42". It is what a time's
 * tooltip shows beside the words of the time itself (#1066). */
export function stamp(when: string): string {
  return new Date(when).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
}

/** The words of a tooltip that say what a time measures. */
export type Measured = "Updated" | "Commented" | "Reviewed" | "Opened" | "Synced";

/** The words of a tooltip: what the time measures, and when it was, as "Updated 23 Nov 2026, 03:42"
 * (#1066). */
export const tip = (measured: Measured, when: string): string => `${measured} ${stamp(when)}`;
