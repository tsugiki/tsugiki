// The status bar's words for what the core waits on, a request to GitHub or `gh` with the token, which it
// shows once the answer is slow.
import type { InFlight } from "$lib/state/status.svelte";
import { SECOND_MS } from "./time";

/** How long a request goes without an answer before the status bar shows it. A sync sends about one a
 * second, and showing each would flicker. */
export const SLOW_MS = 3 * SECOND_MS;

/** What the core asks GitHub. */
type AskingGitHub = Exclude<InFlight["asked"], { kind: "token" }>;

/** What a request asks, as the status bar names it. */
function askingWords(asked: AskingGitHub): string {
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

/** The words for a slow request, in two, so the status bar can cut a long subject and keep the seconds. */
export type InFlightWords = { asking: string; waited: string };

/** What the request in flight asks and the whole seconds it has waited by `now`, in milliseconds, once it
 * has waited [`SLOW_MS`]. Nothing, between requests and while one is quick. */
export function inFlightWords(inFlight: InFlight | null, now: number): InFlightWords | null {
  if (inFlight === null) return null;
  const waited = now - Date.parse(inFlight.since);
  if (waited < SLOW_MS) return null;
  const asked = inFlight.asked;
  return {
    asking:
      asked.kind === "token" ? "Waiting for gh to give the token" : `Waiting for GitHub: ${askingWords(asked)}`,
    waited: `, ${String(Math.floor(waited / SECOND_MS))} s`,
  };
}
