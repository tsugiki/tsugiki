// The status bar's words for the request to GitHub in flight, which it shows once GitHub is slow to answer.
import type { InFlight } from "$lib/state/status.svelte";
import { SECOND_MS } from "./time";

/** How long a request goes without an answer before the status bar shows it. A sync sends about one a
 * second, and showing each would flicker. */
export const SLOW_MS = 3 * SECOND_MS;

/** What a request asks, as the status bar names it. */
function askingWords(asked: InFlight["asked"]): string {
  switch (asked.kind) {
    case "status":
      return "status check";
    case "refresh":
      return `refresh of ${asked.repository}`;
    case "sync":
      return `sync of ${asked.repository}`;
    case "read":
      return `read of ${asked.repository}#${String(asked.number)}`;
    case "fetch":
      return `fetch of ${asked.repository}#${String(asked.number)}`;
    case "check":
      return `check of ${asked.repository}`;
    case "size":
      return `size of ${asked.repository}`;
  }
}

/** What the request in flight asks and the whole seconds it has waited by `now`, in milliseconds, once it
 * has waited [`SLOW_MS`]. Nothing, between requests and while one is quick. */
export function inFlightWords(inFlight: InFlight | null, now: number): string | null {
  if (inFlight === null) return null;
  const waited = now - Date.parse(inFlight.since);
  if (waited < SLOW_MS) return null;
  return `Waiting for GitHub: ${askingWords(inFlight.asked)}, ${String(Math.floor(waited / SECOND_MS))} s`;
}
