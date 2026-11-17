// The status bar's words for the hour's GitHub points, which every tool that uses the token shares.
import type { HourPoints } from "$lib/state/status.svelte";
import { clockTime } from "./synced";

/** A count in the English the words around it are in, as text search writes its counts. The time beside it
 * follows the system's clock, as the user set it. */
const count = (points: number) => points.toLocaleString("en-US");

/** The part of the bar, such as "1,240 points left · resets 14:32". */
export function pointsWords(points: HourPoints): string {
  return `${count(points.remaining)} points left · resets ${clockTime(points.resets_at)}`;
}

/** The part's tooltip. After a restart in the middle of the hour Tsugiki doesn't know what it spent before,
 * so it says since when it counted, and can't say who spent the rest. */
export function pointsTip(points: HourPoints): string {
  const used = `This hour: ${count(points.used)} of ${count(points.limit)} points used.`;
  if (points.since !== null) {
    return `${used} Tsugiki spent ${count(points.spent)} since ${clockTime(points.since)}.`;
  }
  if (points.spent >= points.used) return `${used} Tsugiki spent all of them.`;
  return `${used} Tsugiki spent ${count(points.spent)} of them; other tools using your token spent the rest.`;
}
