// The causes of long frames, for the performance HUD (ADR 0017). Times are milliseconds on the clock of
// `performance.now()`.
import type { Cause } from "$lib/data/bindings";
import type { Frame } from "./frames";

export type { Cause };

/** Something the window did, and when. */
export interface Happening {
  at: number;
  cause: Cause;
}

/**
 * The time before a frame began that can still hold its cause. A key's time comes from the OS, a little
 * before the page hears it.
 */
const LEAD = 50;

/** The last thing that happened while `frame` took its time, or up to `LEAD` before it began. */
export function causeOf(frame: Frame, happenings: readonly Happening[]): Cause | undefined {
  const start = frame.end - frame.duration - LEAD;
  return happenings.findLast((happening) => happening.at >= start && happening.at <= frame.end)?.cause;
}

/** The words for a cause, as the log has them too. */
export function describeCause(cause: Cause | undefined): string {
  switch (cause?.kind) {
    case undefined:
      return "with no known cause";
    case "key":
      return "after a key";
    case "click":
      return "after a click";
    case "view_query":
      return "after a view's answer";
    case "issue_text":
      return `after #${String(cause.number)}'s text arrived from GitHub`;
  }
}
